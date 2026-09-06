using Skopli.Internal;

namespace Skopli;

/// <summary>
/// A pricing handle over a resolved set of catalogs. The full built-in lifecycle
/// (fetch, disk cache, TTL, offline, refresh, stale-fallback) runs in C# and the
/// resolved catalogs are handed down as JSON with <c>builtinSources: false</c>;
/// no callback ever crosses the FFI. Later queries on the same instance reload
/// the catalogs once the in-memory load expires past <c>TtlMs</c>.
///
/// A single private lock guards the native handle across every P/Invoke, the
/// reload handle-swap, and <see cref="Dispose"/>, and the async wrappers acquire
/// that same guard (their <c>Task.Run</c> body calls the locked sync method), so
/// a queued async call cannot race disposal into a double-free or use-after-free.
/// The native handle is freed by <see cref="Dispose"/> (use a
/// <c>using</c> statement).
/// </summary>
public sealed class Pricing : IDisposable
{
    private readonly object _lock = new();
    private readonly PricingOptions _options;
    private readonly IReadOnlyList<PricingSource> _sources;
    private readonly long _ttlMs;
    private readonly Func<DateTimeOffset> _clock;

    // refresh is one-shot: it applies to the first load only. A failed refresh
    // load leaves _pendingRefresh set so the retry honours it; sources whose
    // refresh fetch already completed are tracked so a retry does not refetch
    // them (mirrors src/pricing/index.ts). Guarded by _lock.
    private bool _pendingRefresh;
    private readonly HashSet<PricingSource> _refreshed = new();

    private IntPtr _handle;
    private DateTimeOffset? _loadedAt;
    private bool _disposed;

    private Pricing(PricingOptions options)
    {
        _options = options;
        _sources = options.Sources ?? DefaultSources;
        _ttlMs = options.TtlMs ?? SourceLoader.DefaultTtlMs;
        _clock = options.Clock ?? (() => DateTimeOffset.UtcNow);
        _pendingRefresh = options.Refresh;
    }

    /// <summary>Create a pricing handle, loading any facade-side sources synchronously.</summary>
    internal static Pricing Create(PricingOptions options)
    {
        var pricing = new Pricing(options);
        lock (pricing._lock)
        {
            pricing.Load();
        }
        return pricing;
    }

    /// <summary>Create a pricing handle off-thread, awaiting source loads.</summary>
    internal static Task<Pricing> CreateAsync(PricingOptions options) =>
        Task.Run(() => Create(options));

    /// <summary>The provenance of the catalogs this handle holds.</summary>
    public IReadOnlyList<CatalogInfo> Catalogs()
    {
        lock (_lock)
        {
            EnsureFresh();
            string json = Interop.PricingCatalogInfo(_handle);
            return Json.DeserializeArray<CatalogInfo>(json);
        }
    }

    /// <summary>Price a batch of events grouped by a dimension.</summary>
    public IReadOnlyList<PricedRollup> PriceEvents(IEnumerable<UsageEvent> events, RollupOptions options)
    {
        ArgumentNullException.ThrowIfNull(events);
        ArgumentNullException.ThrowIfNull(options);
        string eventsJson = WireBuilder.Events(events);
        string optsJson = WireBuilder.RollupOptions(options);
        lock (_lock)
        {
            EnsureFresh();
            string json = Interop.PricingPriceEvents(_handle, eventsJson, optsJson);
            return Json.ParsePricedRollups(json);
        }
    }

    /// <summary>Price a batch of events off-thread.</summary>
    public Task<IReadOnlyList<PricedRollup>> PriceEventsAsync(IEnumerable<UsageEvent> events, RollupOptions options) =>
        Task.Run(() => PriceEvents(events, options));

    /// <summary>Price a set of rollups, attaching a hit/miss to each bucket.</summary>
    public IReadOnlyList<PricedRollup> PriceRollups(IEnumerable<Rollup> rollups)
    {
        ArgumentNullException.ThrowIfNull(rollups);
        string rollupsJson = WireBuilder.Rollups(rollups);
        lock (_lock)
        {
            EnsureFresh();
            string json = Interop.PricingPriceRollups(_handle, rollupsJson);
            return Json.ParsePricedRollups(json);
        }
    }

    /// <summary>Price a set of rollups off-thread.</summary>
    public Task<IReadOnlyList<PricedRollup>> PriceRollupsAsync(IEnumerable<Rollup> rollups) =>
        Task.Run(() => PriceRollups(rollups));

    /// <summary>Look up a single model's price (a <see cref="PriceHit"/> or <see cref="PriceMiss"/>).</summary>
    public PriceLookup LookupModel(string model)
    {
        ArgumentException.ThrowIfNullOrEmpty(model);
        lock (_lock)
        {
            EnsureFresh();
            string json = Interop.PricingLookupModel(_handle, model);
            return Json.ParseLookup(json);
        }
    }

    /// <summary>Look up a single model's price off-thread.</summary>
    public Task<PriceLookup> LookupModelAsync(string model) =>
        Task.Run(() => LookupModel(model));

    public void Dispose()
    {
        lock (_lock)
        {
            if (_disposed)
            {
                return;
            }
            _disposed = true;
            if (_handle != IntPtr.Zero)
            {
                Interop.PricingFree(_handle);
                _handle = IntPtr.Zero;
            }
            _loadedAt = null;
        }
    }

    // -----------------------------------------------------------------------

    /// <summary>
    /// Resolve the source catalogs and build a fresh native handle, swapping it
    /// in and freeing the previous one. Must be called with <c>_lock</c> held. A
    /// failed load leaves <c>_loadedAt</c> null (not memoized) so the next query
    /// retries; a failed refresh load leaves <c>_pendingRefresh</c> set so the
    /// retry honours it.
    /// </summary>
    private void Load()
    {
        bool refreshing = _pendingRefresh;
        _pendingRefresh = false;
        _loadedAt = null;

        IReadOnlyList<Catalog> merged;
        try
        {
            merged = ResolveCatalogs(refreshing).GetAwaiter().GetResult();
        }
        catch
        {
            if (refreshing)
            {
                _pendingRefresh = true;
            }
            throw;
        }

        string optsJson = WireBuilder.Pricing(_options.Mode, _options.Overrides, merged);
        IntPtr handle;
        try
        {
            handle = Interop.PricingNew(optsJson);
        }
        catch
        {
            if (refreshing)
            {
                _pendingRefresh = true;
            }
            throw;
        }
        if (_handle != IntPtr.Zero)
        {
            Interop.PricingFree(_handle);
        }
        _handle = handle;
        _loadedAt = _clock();
    }

    /// <summary>
    /// Reload the catalogs when the in-memory load has expired past
    /// <c>_ttlMs</c>. Must be called with <c>_lock</c> held. The comparison is
    /// <c>now - loadedAt &gt;= ttlMs</c> (deliberately distinct from disk-cache
    /// staleness, which is <c>age &gt; ttlMs</c>). A failed load
    /// is not memoized, so the next query retries.
    /// </summary>
    private void EnsureFresh()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        if (_loadedAt is not { } loadedAt)
        {
            Load();
            return;
        }
        long age = _clock().ToUnixTimeMilliseconds() - loadedAt.ToUnixTimeMilliseconds();
        if (age >= _ttlMs)
        {
            Load();
        }
    }

    /// <summary>
    /// Explicit catalogs first, then each source's loaded catalog in priority
    /// order. Priority highest-first is overrides (applied in the wire options),
    /// then explicit catalogs, then the sources in order.
    /// </summary>
    private async Task<IReadOnlyList<Catalog>> ResolveCatalogs(bool refreshing)
    {
        var merged = new List<Catalog>();
        if (_options.Catalogs is not null)
        {
            merged.AddRange(_options.Catalogs);
        }
        if (_sources.Count > 0)
        {
            var loader = new SourceLoader(_options);
            foreach (PricingSource source in _sources)
            {
                // per-source refresh: a retry after a partial failure skips
                // sources whose refresh fetch already completed
                bool refresh = refreshing && !_refreshed.Contains(source);
                Catalog? catalog = await loader.LoadAsync(source, refresh).ConfigureAwait(false);
                if (refresh)
                {
                    _refreshed.Add(source);
                }
                if (catalog is not null)
                {
                    merged.Add(catalog);
                }
            }
        }
        return merged;
    }

    private static readonly IReadOnlyList<PricingSource> DefaultSources = new[]
    {
        PricingSource.OpenRouter(),
        PricingSource.LiteLlm(),
        PricingSource.ModelsDev(),
    };
}

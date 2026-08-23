using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Skopli.Internal;

/// <summary>
/// The facade-side built-in pricing lifecycle for one <see cref="Pricing"/>
/// instance: fetch, byte-compatible disk cache, TTL freshness, offline,
/// one-shot refresh, and stale-fallback, mirroring TS <c>src/pricing/{sources,cache}.ts</c>.
/// Each source's raw payload is routed to the core parser via a
/// <c>{source, fetchedAt, format, payload}</c> catalog; parsers are never
/// reimplemented here.
/// </summary>
internal sealed class SourceLoader
{
    internal const long DefaultTtlMs = 60 * 60 * 1000;
    internal const int FetchTimeoutMs = 10_000;

    // the on-disk cache filename a built-in source reads and writes; the single
    // production formula, exposed for the constants contract test in golden/pricing/lifecycle
    internal static string CacheFileName(string name) => $"pricing-{name}.json";

    // the single read-parse of a cache fetchedAt stamp to epoch-ms (null when
    // unparseable), and the single write-format of epoch-ms to an integral-ms
    // Z-suffixed stamp; both drive the disk-cache read/write path below and the
    // timestamp-boundary contract test exercises them end to end via LoadCached /
    // StoreCached
    internal static long? ParseFetchedAtMs(string fetchedAt) =>
        DateTimeOffset.TryParse(
            fetchedAt, null, System.Globalization.DateTimeStyles.AdjustToUniversal, out DateTimeOffset parsed)
            ? parsed.ToUnixTimeMilliseconds()
            : null;

    internal static string FormatFetchedAt(DateTimeOffset when) =>
        when.UtcDateTime.ToString(
            "yyyy-MM-ddTHH:mm:ss.fffZ", System.Globalization.CultureInfo.InvariantCulture);

    private static readonly Lazy<HttpClient> SharedClient = new(() =>
        new HttpClient { Timeout = TimeSpan.FromMilliseconds(FetchTimeoutMs) });

    private readonly string _cacheDir;
    private readonly bool _offline;
    private readonly long _ttlMs;
    private readonly Func<string, Task<string>> _fetch;
    private readonly Func<DateTimeOffset> _clock;

    internal SourceLoader(PricingOptions options)
    {
        _cacheDir = options.CacheDir ?? Interop.DefaultCacheDir();
        _offline = options.Offline;
        _ttlMs = options.TtlMs ?? DefaultTtlMs;
        _fetch = options.Fetch ?? DefaultFetch;
        _clock = options.Clock ?? (() => DateTimeOffset.UtcNow);
    }

    /// <summary>
    /// Load one source's catalog, mirroring TS <c>src/pricing/sources.ts</c>. A
    /// cached payload that probes to zero prices is unusable (treated as no
    /// cache); a fetched payload is probed BEFORE the cache is written, and an
    /// invalid-JSON or zero-price body is treated as a fetch failure (serve the
    /// stale catalog if one probed usable, else skip). <paramref name="refresh"/>
    /// forces one fresh fetch, bypassing a fresh disk cache.
    /// </summary>
    internal async Task<Catalog?> LoadAsync(PricingSource source, bool refresh)
    {
        string cacheName = CacheFileName(source.Name);
        Cached? cached = LoadCached(cacheName);
        // a cache payload that probes to zero prices is unusable, same as no cache
        Catalog? cachedCatalog = null;
        if (cached is not null)
        {
            Catalog candidate = Wire(source, cached.FetchedAt, cached.Payload);
            if (Probe.Usable(source.Name, candidate))
            {
                cachedCatalog = candidate;
            }
        }
        bool cacheFresh = cached is { Stale: false } && !refresh;
        if (cachedCatalog is not null && (cacheFresh || _offline))
        {
            return cachedCatalog;
        }
        if (_offline)
        {
            return null;
        }
        string payload;
        try
        {
            payload = await _fetch(source.Url).ConfigureAwait(false);
        }
        catch
        {
            // any-age stale fallback keeps pricing available when a fetch fails
            return cachedCatalog;
        }
        // probe the fetched payload BEFORE writing the cache: an invalid-JSON or
        // zero-price body is a fetch failure, so serve the stale catalog (or skip)
        // and never overwrite the cache. Write-then-parse would be the bug.
        string wouldStamp = _clock().UtcDateTime.ToString(
            "yyyy-MM-ddTHH:mm:ss.fffZ", System.Globalization.CultureInfo.InvariantCulture);
        Catalog fetched = Wire(source, wouldStamp, payload);
        if (!Probe.Usable(source.Name, fetched))
        {
            return cachedCatalog;
        }
        string fetchedAt = StoreCached(cacheName, payload);
        return Wire(source, fetchedAt, payload);
    }

    private static Catalog Wire(PricingSource source, string fetchedAt, string payload) => new()
    {
        Source = source.Name,
        FetchedAt = fetchedAt,
        Format = source.Format,
        Payload = payload,
    };

    private sealed record Cached(string FetchedAt, string Payload, bool Stale);

    private Cached? LoadCached(string name)
    {
        string path = Path.Combine(_cacheDir, name);
        string raw;
        try
        {
            raw = File.ReadAllText(path);
        }
        catch
        {
            return null;
        }
        JsonObject? file;
        try
        {
            file = JsonNode.Parse(raw)?.AsObject();
        }
        catch
        {
            return null;
        }
        if (file is null)
        {
            return null;
        }
        if (file["fetchedAt"] is not JsonValue fetchedAtNode ||
            !fetchedAtNode.TryGetValue(out string? fetchedAt) ||
            file["payload"] is not JsonNode payloadNode)
        {
            return null;
        }
        if (ParseFetchedAtMs(fetchedAt) is not long fetchedMs)
        {
            return null;
        }
        long age = _clock().ToUnixTimeMilliseconds() - fetchedMs;
        return new Cached(fetchedAt, payloadNode.ToJsonString(), age > _ttlMs);
    }

    private string StoreCached(string name, string payload)
    {
        string fetchedAt = FormatFetchedAt(_clock());
        var file = new JsonObject
        {
            ["fetchedAt"] = fetchedAt,
            ["payload"] = JsonNode.Parse(payload),
        };
        Directory.CreateDirectory(_cacheDir);
        string target = Path.Combine(_cacheDir, name);
        // write-then-rename so a concurrent reader never sees a torn file
        string tmp = Path.Combine(
            _cacheDir, $"{name}.{Environment.ProcessId}.{Guid.NewGuid():N}.tmp");
        File.WriteAllText(tmp, file.ToJsonString(), new UTF8Encoding(false));
        try
        {
            File.Move(tmp, target, overwrite: true);
        }
        catch
        {
            try { File.Delete(tmp); } catch { /* best effort */ }
            throw;
        }
        return fetchedAt;
    }

    private static async Task<string> DefaultFetch(string url)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, url);
        using HttpResponseMessage response = await SharedClient.Value
            .SendAsync(request, HttpCompletionOption.ResponseHeadersRead)
            .ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            throw new HttpRequestException($"{url} responded {(int)response.StatusCode}");
        }
        return await response.Content.ReadAsStringAsync().ConfigureAwait(false);
    }
}

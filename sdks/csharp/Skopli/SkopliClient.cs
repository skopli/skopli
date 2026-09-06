using Skopli.Internal;

namespace Skopli;

/// <summary>
/// The idiomatic C# entry point over the skopli C ABI. Static operations
/// mirror the common facade contract: detect / read / rollup / cost /
/// pricing. The sync methods are the primitive; the <c>*Async</c> conveniences run
/// the sync core off-thread via <see cref="Task.Run(System.Action)"/>, matching
/// the .NET async-first expectation for I/O APIs. The path seam travels as data
/// (<see cref="PathOptions"/> -> <c>{home, env}</c>); pricing sources run
/// facade-side (see <see cref="Pricing"/>); no callback ever crosses the FFI.
/// </summary>
public static class SkopliClient
{
    /// <summary>The native library semver string.</summary>
    public static string Version => Interop.Version();

    /// <summary>The ABI version integer (bumped on a breaking ABI change).</summary>
    public static uint AbiVersion => Interop.AbiVersion();

    /// <summary>The conformance schema_version this build emits.</summary>
    public static uint SchemaVersion => Interop.SchemaVersion();

    /// <summary>The platform default on-disk cache directory.</summary>
    public static string DefaultCacheDir() => Interop.DefaultCacheDir();

    /// <summary>Detect which registered harnesses have readable data.</summary>
    public static Detection DetectHarnesses(DetectOptions? options = null)
    {
        string json = Interop.DetectHarnesses(WireBuilder.PathOptions(options));
        return Json.Deserialize<Detection>(json);
    }

    /// <summary>Read usage across the selected harnesses.</summary>
    public static ReadUsageResult ReadUsage(ReadUsageOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        string json = Interop.ReadUsage(WireBuilder.ReadUsage(options));
        return Json.Deserialize<ReadUsageResult>(json);
    }

    /// <summary>Read usage off-thread.</summary>
    public static Task<ReadUsageResult> ReadUsageAsync(ReadUsageOptions options) =>
        Task.Run(() => ReadUsage(options));

    /// <summary>Roll up a batch of events by a dimension.</summary>
    public static IReadOnlyList<Rollup> Rollup(IEnumerable<UsageEvent> events, RollupOptions options)
    {
        ArgumentNullException.ThrowIfNull(events);
        ArgumentNullException.ThrowIfNull(options);
        string json = Interop.Rollup(WireBuilder.Events(events), WireBuilder.RollupOptions(options));
        return Json.DeserializeArray<Rollup>(json);
    }

    /// <summary>Compute the USD cost of a token bundle under a flat/tiered price.</summary>
    public static double CostUsd(TokenCounts tokens, ModelPrice price)
    {
        ArgumentNullException.ThrowIfNull(tokens);
        ArgumentNullException.ThrowIfNull(price);
        return Interop.CostUsd(WireBuilder.TokenCounts(tokens), WireBuilder.ModelPrice(price));
    }

    /// <summary>Create a <see cref="Pricing"/> handle, resolving any facade-side sources.</summary>
    public static Pricing CreatePricing(PricingOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        return Pricing.Create(options);
    }

    /// <summary>Create a <see cref="Pricing"/> handle off-thread (source loads run async).</summary>
    public static Task<Pricing> CreatePricingAsync(PricingOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        return Pricing.CreateAsync(options);
    }
}

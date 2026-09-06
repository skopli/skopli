namespace Skopli;

/// <summary>
/// Path-resolution seam as plain data: the facade flattens any
/// host-side resolver abstraction to <c>{home, env}</c> before the FFI call.
/// Both null means "process defaults".
/// </summary>
public record PathOptions
{
    /// <summary>Override the home directory used to locate harness data.</summary>
    public string? Home { get; init; }

    /// <summary>Override the environment map used to locate harness data.</summary>
    public IReadOnlyDictionary<string, string>? Env { get; init; }
}

/// <summary>Options for <c>DetectHarnesses</c> - just the path seam.</summary>
public sealed record DetectOptions : PathOptions
{
}

/// <summary>How subagent events are treated when reading usage.</summary>
public enum SubagentMode
{
    Include,
    Exclude,
}

/// <summary>Options for <c>ReadUsage</c> - the path seam plus harness/date/tz filters.</summary>
public sealed record ReadUsageOptions : PathOptions
{
    /// <summary>Restrict to these harness ids; null = detect all.</summary>
    public IReadOnlyList<string>? Harnesses { get; init; }

    /// <summary>Inclusive lower bound (a <c>YYYY-MM-DD</c> or ISO timestamp).</summary>
    public string? Since { get; init; }

    /// <summary>Exclusive upper bound (a <c>YYYY-MM-DD</c> or ISO timestamp).</summary>
    public string? Until { get; init; }

    /// <summary>Timezone for date-only bounds and day bucketing (default UTC).</summary>
    public string? Tz { get; init; }

    /// <summary>Whether to include or exclude subagent events (default include).</summary>
    public SubagentMode Subagents { get; init; } = SubagentMode.Include;
}

/// <summary>Options for <c>Rollup</c> / event pricing - the grouping dimension and tz.</summary>
public sealed record RollupOptions
{
    /// <summary>The default billing-block width (five hours) for <see cref="RollupBy.Block"/>.</summary>
    public const long DefaultBlockMs = 18_000_000;

    public required RollupBy By { get; init; }

    public string? Tz { get; init; }

    /// <summary>Billing-block width in milliseconds for <see cref="RollupBy.Block"/>; null uses <see cref="DefaultBlockMs"/>.</summary>
    public long? BlockMs { get; init; }
}

/// <summary>The costing mode a <see cref="Pricing"/> handle applies.</summary>
public enum PricingMode
{
    /// <summary>Recompute cost from tokens x catalog price.</summary>
    Calculate,

    /// <summary>Use the recorded cost when present, else calculate.</summary>
    Auto,

    /// <summary>Use only the recorded cost.</summary>
    Display,
}

/// <summary>A programmatic price override for a single model (highest priority).</summary>
public sealed record PriceOverride
{
    public required string Model { get; init; }

    public required double Input { get; init; }

    public required double Output { get; init; }

    public double? CacheRead { get; init; }

    public double? CacheWrite { get; init; }

    public double? CacheWrite1h { get; init; }
}

/// <summary>
/// A pre-fetched pricing catalog handed down to the core. Supply
/// either <see cref="Prices"/> (already-parsed model prices) or a raw
/// <see cref="Format"/> + <see cref="Payload"/> pair the core will parse.
/// </summary>
public sealed record Catalog
{
    public required string Source { get; init; }

    public string? FetchedAt { get; init; }

    /// <summary>Already-parsed model prices; mutually exclusive with <see cref="Format"/>.</summary>
    public IReadOnlyDictionary<string, ModelPrice>? Prices { get; init; }

    /// <summary>A raw payload format ("openrouter" | "litellm" | "modelsdev").</summary>
    public string? Format { get; init; }

    /// <summary>The raw source JSON payload (as text) paired with <see cref="Format"/>.</summary>
    public string? Payload { get; init; }
}

/// <summary>
/// A built-in market pricing source: a name, its fetch URL, and the core parser
/// format for its raw payload. The lifecycle (fetch, cache, TTL, offline,
/// refresh, stale-fallback) runs entirely facade-side; the raw payload is routed
/// to the core parser via a <c>{source, fetchedAt, format, payload}</c> catalog.
/// </summary>
public sealed record PricingSource
{
    /// <summary>The source name; also the cache file stem (<c>pricing-&lt;name&gt;.json</c>).</summary>
    public required string Name { get; init; }

    /// <summary>The endpoint the raw payload is fetched from.</summary>
    public required string Url { get; init; }

    /// <summary>The core parser format ("openrouter" | "litellm" | "modelsdev").</summary>
    public required string Format { get; init; }

    /// <summary>OpenRouter: <c>https://openrouter.ai/api/v1/models</c>.</summary>
    public static PricingSource OpenRouter(string url = "https://openrouter.ai/api/v1/models") =>
        new() { Name = "openrouter", Url = url, Format = "openrouter" };

    /// <summary>LiteLLM: the BerriAI model-prices JSON.</summary>
    public static PricingSource LiteLlm(
        string url = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json") =>
        new() { Name = "litellm", Url = url, Format = "litellm" };

    /// <summary>models.dev: <c>https://models.dev/api.json</c>.</summary>
    public static PricingSource ModelsDev(string url = "https://models.dev/api.json") =>
        new() { Name = "models-dev", Url = url, Format = "modelsdev" };
}

/// <summary>Options for <c>CreatePricing</c>. Mirrors TS <c>CreatePricingOptions</c>.</summary>
public sealed record PricingOptions
{
    /// <summary>The costing mode (default <see cref="PricingMode.Calculate"/>).</summary>
    public PricingMode Mode { get; init; } = PricingMode.Calculate;

    /// <summary>Programmatic per-model overrides (highest priority).</summary>
    public IReadOnlyList<PriceOverride>? Overrides { get; init; }

    /// <summary>
    /// Priority-ordered built-in market sources; null defaults to
    /// OpenRouter &gt; LiteLLM &gt; models.dev. An empty list disables all built-ins.
    /// </summary>
    public IReadOnlyList<PricingSource>? Sources { get; init; }

    /// <summary>Pre-fetched catalogs handed straight down (below overrides, above built-ins).</summary>
    public IReadOnlyList<Catalog>? Catalogs { get; init; }

    /// <summary>The on-disk cache directory; null resolves to the platform default.</summary>
    public string? CacheDir { get; init; }

    /// <summary>When true, never fetch: serve a present cache, else skip the source.</summary>
    public bool Offline { get; init; }

    /// <summary>Cache freshness window in ms; null defaults to 1 hour (3_600_000).</summary>
    public long? TtlMs { get; init; }

    /// <summary>Force one fresh fetch per source, with stale-cache fallback on failure.</summary>
    public bool Refresh { get; init; }

    /// <summary>Injected fetcher returning the raw source JSON text; null uses HttpClient.</summary>
    public Func<string, Task<string>>? Fetch { get; init; }

    /// <summary>Clock seam for deterministic tests; null uses the system UTC clock.</summary>
    public Func<DateTimeOffset>? Clock { get; init; }
}

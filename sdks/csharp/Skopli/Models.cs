using System.Text.Json.Serialization;

namespace Skopli;

/// <summary>
/// A coding-agent harness. String values match the core's harness ids exactly;
/// <see cref="Other"/> absorbs any id a newer core emits so an upgraded native
/// library cannot break the facade.
/// </summary>
public enum Harness
{
    Other = 0,
    Claude,
    Codex,
    Gemini,
    Opencode,
    Mimocode,
    Commandcode,
    Amp,
    Copilot,
    Cline,
    Roo,
    Kilo,
    Kilocode,
    Augment,
    Codebuff,
    Codebuddy,
    Jcode,
    Droid,
    Goose,
    Junie,
    Kimi,
    Mux,
    Openclaw,
    Omp,
    Pi,
    Prime,
    Gajae,
    Kimchi,
    Grok,
    Qwen,
    Zcode,
    Devin,
    Hermes,
    Zed,
    Cherrystudio,
    Opencodereview,
    Trae,
    Deepseek,
    Reasonix,
    Kiro,
    Fx,
}

/// <summary>The dimension a rollup or event-pricing groups by.</summary>
public enum RollupBy
{
    Model,
    Day,
    Session,
    Harness,
    Workspace,
    Block,
}

/// <summary>Token counts for a single usage event or an aggregated group.</summary>
public sealed record TokenCounts
{
    [JsonPropertyName("input")]
    public long Input { get; init; }

    [JsonPropertyName("output")]
    public long Output { get; init; }

    [JsonPropertyName("cacheRead")]
    public long CacheRead { get; init; }

    [JsonPropertyName("cacheWrite")]
    public long CacheWrite { get; init; }

    [JsonPropertyName("cacheWrite1h")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public long? CacheWrite1h { get; init; }

    [JsonPropertyName("reasoning")]
    public long Reasoning { get; init; }
}

/// <summary>A single usage event read from a harness's local data.</summary>
public sealed record UsageEvent
{
    [JsonPropertyName("harness")]
    public required string Harness { get; init; }

    [JsonPropertyName("timestamp")]
    public required string Timestamp { get; init; }

    [JsonPropertyName("sessionId")]
    public required string SessionId { get; init; }

    [JsonPropertyName("messageId")]
    public required string MessageId { get; init; }

    [JsonPropertyName("turn")]
    public bool Turn { get; init; }

    [JsonPropertyName("subagent")]
    public bool Subagent { get; init; }

    [JsonPropertyName("model")]
    public required string Model { get; init; }

    [JsonPropertyName("tokens")]
    public required TokenCounts Tokens { get; init; }

    [JsonPropertyName("calls")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public long? Calls { get; init; }

    [JsonPropertyName("costUsd")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CostUsd { get; init; }

    [JsonPropertyName("workspace")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Workspace { get; init; }

    [JsonPropertyName("title")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Title { get; init; }
}

/// <summary>A non-fatal note produced while reading (never an error).</summary>
public sealed record Diagnostic
{
    [JsonPropertyName("severity")]
    public required string Severity { get; init; }

    [JsonPropertyName("message")]
    public required string Message { get; init; }

    [JsonPropertyName("harness")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Harness { get; init; }
}

/// <summary>The <c>{events, diagnostics, skipped}</c> envelope from <c>ReadUsage</c>.</summary>
public sealed record ReadUsageResult
{
    [JsonPropertyName("events")]
    public IReadOnlyList<UsageEvent> Events { get; init; } = Array.Empty<UsageEvent>();

    [JsonPropertyName("diagnostics")]
    public IReadOnlyList<Diagnostic> Diagnostics { get; init; } = Array.Empty<Diagnostic>();

    [JsonPropertyName("skipped")]
    public IReadOnlyDictionary<string, IReadOnlyList<string>> Skipped { get; init; } =
        new Dictionary<string, IReadOnlyList<string>>();
}

/// <summary>Which harnesses have readable data (supported) vs have no ABI representation.</summary>
public sealed record Detection
{
    [JsonPropertyName("supported")]
    public IReadOnlyList<string> Supported { get; init; } = Array.Empty<string>();

    [JsonPropertyName("unsupported")]
    public IReadOnlyList<string> Unsupported { get; init; } = Array.Empty<string>();
}

/// <summary>One rollup bucket: the grouping key plus aggregated counters.</summary>
public sealed record Rollup
{
    [JsonPropertyName("key")]
    public required string Key { get; init; }

    [JsonPropertyName("tokens")]
    public required TokenCounts Tokens { get; init; }

    [JsonPropertyName("events")]
    public long Events { get; init; }

    [JsonPropertyName("turns")]
    public long Turns { get; init; }

    [JsonPropertyName("calls")]
    public long Calls { get; init; }

    [JsonPropertyName("costUsd")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CostUsd { get; init; }
}

/// <summary>A single price tier (threshold + per-token rates).</summary>
public sealed record PriceTier
{
    [JsonPropertyName("threshold")]
    public double Threshold { get; init; }

    [JsonPropertyName("input")]
    public double Input { get; init; }

    [JsonPropertyName("output")]
    public double Output { get; init; }

    [JsonPropertyName("cacheRead")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CacheRead { get; init; }

    [JsonPropertyName("cacheWrite")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CacheWrite { get; init; }

    [JsonPropertyName("cacheWrite1h")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CacheWrite1h { get; init; }
}

/// <summary>The price for a model: flat rates plus optional cache rates and tiers.</summary>
public sealed record ModelPrice
{
    [JsonPropertyName("input")]
    public double Input { get; init; }

    [JsonPropertyName("output")]
    public double Output { get; init; }

    [JsonPropertyName("cacheRead")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CacheRead { get; init; }

    [JsonPropertyName("cacheWrite")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CacheWrite { get; init; }

    [JsonPropertyName("cacheWrite1h")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? CacheWrite1h { get; init; }

    [JsonPropertyName("tierMode")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? TierMode { get; init; }

    [JsonPropertyName("tiers")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<PriceTier>? Tiers { get; init; }
}

/// <summary>Provenance of one catalog held by a <see cref="Pricing"/> handle.</summary>
public sealed record CatalogInfo
{
    [JsonPropertyName("source")]
    public required string Source { get; init; }

    [JsonPropertyName("fetchedAt")]
    public string? FetchedAt { get; init; }

    [JsonPropertyName("models")]
    public long Models { get; init; }
}

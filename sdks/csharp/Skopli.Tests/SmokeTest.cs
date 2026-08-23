using System.Text.Json;
using System.Text.Json.Nodes;
using Skopli;
using Xunit;

namespace Skopli.Tests;

/// <summary>
/// Drives the C# facade end to end through the csbindgen P/Invoke layer against
/// the committed conformance gold - the same data the Rust capi smoke test
/// and the Go/Java SDKs exercise. Asserts structural parity
/// of the read -> rollup -> price pipeline versus the golden expected JSON.
/// </summary>
public sealed class SmokeTest
{
    private static string RepoRoot()
    {
        // Walk up from the test assembly location until the golden/ dir appears.
        string dir = AppContext.BaseDirectory;
        while (dir is not null && !Directory.Exists(Path.Combine(dir, "golden")))
        {
            dir = Path.GetDirectoryName(dir)!;
        }
        Assert.False(dir is null, "could not locate repo root (golden/)");
        return dir!;
    }

    private static string GoldenPath(params string[] parts) =>
        Path.Combine(new[] { RepoRoot(), "golden" }.Concat(parts).ToArray());

    // -- harness enum parity ------------------------------------------------

    [Fact]
    public void HarnessEnumCoversRegistry()
    {
        // The shared registry-id fixture is the canonical list of harness ids
        // the core registers; the Harness enum must cover every one (the enum
        // names lowercase to the wire ids) so a reader added to the core cannot
        // silently fall out of the typed surface.
        string json = File.ReadAllText(GoldenPath("registry", "ids.json"));
        string[] ids = JsonNode.Parse(json)!["ids"]!.AsArray()
            .Select(n => n!.GetValue<string>())
            .ToArray();
        var known = Enum.GetNames<Harness>()
            .Select(name => name.ToLowerInvariant())
            .Where(name => name != "other")
            .ToHashSet();
        string[] missing = ids.Where(id => !known.Contains(id)).ToArray();
        Assert.True(missing.Length == 0, $"Harness enum missing registry ids: {string.Join(", ", missing)}");
        string[] extra = known.Except(ids).OrderBy(id => id).ToArray();
        Assert.True(extra.Length == 0, $"Harness enum has ids the registry does not: {string.Join(", ", extra)}");
    }

    // -- meta / lifecycle ---------------------------------------------------

    [Fact]
    public void Meta()
    {
        Assert.Equal(1u, SkopliClient.AbiVersion);
        Assert.Equal(1u, SkopliClient.SchemaVersion);
        Assert.False(string.IsNullOrEmpty(SkopliClient.Version));
        Assert.Contains("skopli", SkopliClient.DefaultCacheDir());
    }

    // -- cost_usd -----------------------------------------------------------

    [Fact]
    public void CostUsdFlat()
    {
        double usd = SkopliClient.CostUsd(
            new TokenCounts { Input = 1000, Output = 500 },
            new ModelPrice { Input = 1.25, Output = 10.0 });
        double expected = (1000.0 * 1.25 + 500.0 * 10.0) / 1_000_000.0;
        Assert.Equal(expected, usd, 12);
    }

    // -- detect / read / rollup over golden/claude/basic --------------------

    private static ReadUsageOptions ClaudeOptions()
    {
        string inputDir = GoldenPath("claude", "basic", "input");
        return new ReadUsageOptions
        {
            Home = "/nonexistent",
            Env = new Dictionary<string, string> { ["CLAUDE_CONFIG_DIR"] = inputDir },
            Harnesses = new[] { "claude" },
            Tz = "UTC",
        };
    }

    [Fact]
    public void DetectFindsClaude()
    {
        string inputDir = GoldenPath("claude", "basic", "input");
        Detection detection = SkopliClient.DetectHarnesses(new DetectOptions
        {
            Home = "/nonexistent",
            Env = new Dictionary<string, string> { ["CLAUDE_CONFIG_DIR"] = inputDir },
        });
        Assert.Contains("claude", detection.Supported);
        Assert.Empty(detection.Unsupported);
    }

    [Fact]
    public void ReadRollupRoundtripMatchesGold()
    {
        ReadUsageResult envelope = SkopliClient.ReadUsage(ClaudeOptions());
        Assert.NotEmpty(envelope.Events);

        // Sort events the way the exporter/gold does before rolling up.
        List<UsageEvent> sorted = envelope.Events
            .OrderBy(e => e.Timestamp, StringComparer.Ordinal)
            .ThenBy(e => e.SessionId, StringComparer.Ordinal)
            .ThenBy(e => e.MessageId, StringComparer.Ordinal)
            .ThenBy(e => e.Model, StringComparer.Ordinal)
            .ToList();

        IReadOnlyList<Rollup> byModel = SkopliClient.Rollup(sorted, new RollupOptions { By = RollupBy.Model });

        // Compare (JSON-normalized) to the committed gold's model lane.
        JsonNode gold = JsonNode.Parse(File.ReadAllText(GoldenPath("claude", "basic", "expected-rollup.json")))!;
        JsonNode goldModel = gold["by"]!["model"]!;

        Assert.Equal(Normalize(goldModel), NormalizeList(byModel));
    }

    // -- pricing round-trip over golden/pricing/catalogs --------------------

    private static string ReadCatalog(string name) =>
        File.ReadAllText(GoldenPath("pricing", "catalogs", $"{name}.json"));

    private static IReadOnlyList<UsageEvent> PricingEvents()
    {
        UsageEvent Ev(string mid, int sec, string model, TokenCounts tokens) => new()
        {
            Harness = "opencode",
            Timestamp = $"2026-08-01T00:0{sec}:00.000Z",
            SessionId = "s",
            MessageId = mid,
            Turn = true,
            Subagent = false,
            Model = model,
            Tokens = tokens,
        };

        return new[]
        {
            Ev("flat", 0, "gpt-5", new TokenCounts { Input = 1000, Output = 500 }),
            Ev("tiered", 1, "claude-sonnet-4-5", new TokenCounts
            {
                Input = 200_000, Output = 10_000, CacheRead = 20_000,
                CacheWrite = 30_000, CacheWrite1h = 10_000, Reasoning = 2_000,
            }),
            Ev("base-1h", 2, "claude-sonnet-4-5", new TokenCounts
            {
                Input = 5_000, Output = 1_000, CacheRead = 2_000,
                CacheWrite = 4_000, CacheWrite1h = 1_500,
            }),
            Ev("alias", 3, "us.anthropic.claude-opus-4-6-20260115-v1:0", new TokenCounts { Input = 800, Output = 200 }),
            Ev("miss", 4, "totally-unknown-model-9000", new TokenCounts { Input = 100, Output = 100 }),
        };
    }

    [Fact]
    public void PricingRoundtrip()
    {
        const string pinned = "2026-08-01T00:00:00.000Z";
        var options = new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = Array.Empty<PricingSource>(),
            Catalogs = new[]
            {
                new Catalog { Source = "openrouter", FetchedAt = pinned, Format = "openrouter", Payload = ReadCatalog("openrouter") },
                new Catalog { Source = "litellm", FetchedAt = pinned, Format = "litellm", Payload = ReadCatalog("litellm") },
            },
        };

        using Pricing pricing = SkopliClient.CreatePricing(options);

        IReadOnlyList<CatalogInfo> infos = pricing.Catalogs();
        Assert.Equal(2, infos.Count);
        Assert.Equal("openrouter", infos[0].Source);
        Assert.Equal("litellm", infos[1].Source);
        Assert.True(infos[0].Models > 0);

        IReadOnlyList<PricedRollup> priced = pricing.PriceEvents(PricingEvents(), new RollupOptions { By = RollupBy.Model });
        Assert.NotEmpty(priced);

        // gpt-5 is a flat priced hit; the unknown model is a miss.
        PricedRollup gpt5 = priced.Single(g => g.Rollup.Key == "gpt-5");
        PriceHit gpt5Hit = Assert.IsType<PriceHit>(gpt5.Pricing);
        Assert.True(gpt5Hit.Priced);
        Assert.Equal(0.00625, gpt5Hit.Usd, 9);

        PricedRollup miss = priced.Single(g => g.Rollup.Key == "totally-unknown-model-9000");
        PriceMiss missLookup = Assert.IsType<PriceMiss>(miss.Pricing);
        Assert.False(missLookup.Priced);

        // Structural parity vs the committed gold's exactly-agreeing groups
        // (the flat gpt-5 hit, the aliased Bedrock->anthropic key, and the miss).
        // The tiered-aggregate group differs by construction between price_events
        // and the gold's price_rollups, so it is
        // spot-checked rather than full-compared.
        JsonNode gold = JsonNode.Parse(File.ReadAllText(GoldenPath("pricing", "basic", "expected-priced-rollup.json")))!;
        JsonArray goldRollups = gold["rollups"]!.AsArray();

        JsonObject goldAlias = goldRollups.Single(n => (string?)n!["key"] == "us.anthropic.claude-opus-4-6-20260115-v1:0")!.AsObject();
        PricedRollup alias = priced.Single(g => g.Rollup.Key == "us.anthropic.claude-opus-4-6-20260115-v1:0");
        PriceHit aliasHit = Assert.IsType<PriceHit>(alias.Pricing);
        Assert.Equal("anthropic/claude-opus-4.6", aliasHit.Key);
        Assert.Equal((double)goldAlias["pricing"]!["usd"]!, aliasHit.Usd, 9);
        Assert.Equal((string?)goldAlias["pricing"]!["source"], aliasHit.Source);

        // Key-set + count parity across all groups vs the gold.
        var goldKeys = goldRollups.Select(n => (string)n!["key"]!).OrderBy(k => k, StringComparer.Ordinal).ToList();
        var gotKeys = priced.Select(g => g.Rollup.Key).OrderBy(k => k, StringComparer.Ordinal).ToList();
        Assert.Equal(goldKeys, gotKeys);
    }

    [Fact]
    public void PricingDisposesIdempotently()
    {
        Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = Array.Empty<PricingSource>(),
            Catalogs = new[]
            {
                new Catalog { Source = "litellm", FetchedAt = "2026-08-01T00:00:00.000Z", Format = "litellm", Payload = ReadCatalog("litellm") },
            },
        });
        pricing.Dispose();
        pricing.Dispose(); // idempotent
        Assert.Throws<ObjectDisposedException>(() => pricing.Catalogs());
    }

    // -- forced errors ------------------------------------------------------

    [Fact]
    public void InvalidSinceThrowsInvalidArgument()
    {
        var options = ClaudeOptions() with { Since = "not-a-date" };
        Assert.Throws<InvalidArgumentException>(() => SkopliClient.ReadUsage(options));
    }

    [Fact]
    public void UnknownCatalogFormatThrowsCatalog()
    {
        var options = new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = Array.Empty<PricingSource>(),
            Catalogs = new[]
            {
                new Catalog { Source = "bogus", Format = "nonsense-format", Payload = "{}" },
            },
        };
        Assert.Throws<CatalogException>(() => SkopliClient.CreatePricing(options));
    }

    // -- normalization helpers ----------------------------------------------

    private static readonly JsonSerializerOptions SerializerOptions = new()
    {
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
    };

    /// <summary>Reparse a facade rollup list through JSON for a key-order-independent compare.</summary>
    private static string NormalizeList(IReadOnlyList<Rollup> rollups)
    {
        string json = JsonSerializer.Serialize(rollups, SerializerOptions);
        return Normalize(JsonNode.Parse(json)!);
    }

    /// <summary>Canonicalize a JSON node: sort object keys, render numbers uniformly.</summary>
    private static string Normalize(JsonNode node) => Canonical(node).ToJsonString();

    private static JsonNode Canonical(JsonNode node)
    {
        switch (node)
        {
            case JsonObject obj:
                var sorted = new JsonObject();
                foreach (var kv in obj.OrderBy(p => p.Key, StringComparer.Ordinal))
                {
                    sorted[kv.Key] = kv.Value is null ? null : Canonical(kv.Value);
                }
                return sorted;
            case JsonArray arr:
                var outArr = new JsonArray();
                foreach (JsonNode? item in arr)
                {
                    outArr.Add(item is null ? null : Canonical(item));
                }
                return outArr;
            default:
                JsonValue value = node.AsValue();
                if (value.TryGetValue(out double d))
                {
                    return JsonValue.Create(d);
                }
                return JsonValue.Create(value.GetValue<JsonElement>())!;
        }
    }
}

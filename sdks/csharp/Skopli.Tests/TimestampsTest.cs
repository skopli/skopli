using System.Text.Json;
using System.Text.Json.Nodes;
using Skopli;
using Xunit;

namespace Skopli.Tests;

/// <summary>
/// Cache fetchedAt boundary conformance, driven by the SHARED fixtures in
/// <c>golden/pricing/timestamps/</c>. The stamp is one wire contract read and
/// written by seven facades; every case is consumed through this facade's own
/// PRODUCTION cache path (<c>SourceLoader.LoadCached</c> / <c>StoreCached</c>,
/// reached via <c>SkopliClient.CreatePricing</c> with an injected clock and
/// fetch stub) so C# and the native core agree on every boundary.
/// </summary>
public sealed class TimestampsTest
{
    private const string SourceName = "openrouter";
    private const string CacheFile = "pricing-openrouter.json";
    private const string SourceUrl = "https://or.test/models";

    private static string RepoRoot()
    {
        string dir = AppContext.BaseDirectory;
        while (dir is not null && !Directory.Exists(Path.Combine(dir, "golden")))
        {
            dir = Path.GetDirectoryName(dir)!;
        }
        Assert.False(dir is null, "could not locate repo root (golden/)");
        return dir!;
    }

    private static JsonElement Cases()
    {
        string path = Path.Combine(RepoRoot(), "golden", "pricing", "timestamps", "cases.json");
        return JsonDocument.Parse(File.ReadAllText(path)).RootElement;
    }

    // the canonical OpenRouter payload that probes to usable prices, so a loaded
    // or fetched cache is a real catalog and drives the actual lifecycle
    private static string SourcePayload() =>
        JsonNode.Parse(File.ReadAllText(
            Path.Combine(RepoRoot(), "golden", "pricing", "lifecycle", "source-openrouter.json")))!
            .ToJsonString();

    public static IEnumerable<object[]> ReadCases()
    {
        foreach (JsonElement kase in Cases().GetProperty("read").EnumerateArray())
        {
            string name = kase.GetProperty("name").GetString()!;
            string stamp = kase.GetProperty("stamp").GetString()!;
            JsonElement epoch = kase.GetProperty("epochMs");
            long? epochMs = epoch.ValueKind == JsonValueKind.Null ? null : epoch.GetInt64();
            yield return new object[] { name, stamp, epochMs! };
        }
    }

    public static IEnumerable<object[]> WriteCases()
    {
        foreach (JsonElement ms in Cases().GetProperty("write").EnumerateArray())
        {
            yield return new object[] { ms.GetInt64() };
        }
    }

    // a fetch stub that records calls and serves the canonical payload
    private sealed class RecordingFetch
    {
        private readonly string _payload;

        public RecordingFetch(string payload) => _payload = payload;

        public int Calls { get; private set; }

        public Func<string, Task<string>> Fetch => _ =>
        {
            Calls++;
            return Task.FromResult(_payload);
        };
    }

    private static string TempDir()
    {
        string dir = Path.Combine(Path.GetTempPath(), "skopli-csharp-timestamps-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        return dir;
    }

    private static void WriteCacheBody(string dir, string stamp) =>
        File.WriteAllText(
            Path.Combine(dir, CacheFile),
            new JsonObject
            {
                ["fetchedAt"] = stamp,
                ["payload"] = JsonNode.Parse(SourcePayload()),
            }.ToJsonString());

    private static string? PersistedFetchedAt(string dir)
    {
        string path = Path.Combine(dir, CacheFile);
        if (!File.Exists(path))
        {
            return null;
        }
        return (string)JsonNode.Parse(File.ReadAllText(path))!.AsObject()["fetchedAt"]!;
    }

    // Drive the production lifecycle once at a fixed clock instant with ttl = 0
    // and a disk cache already present. LoadCached runs; if it reports the cache
    // fresh no fetch fires and the persisted fetchedAt is unchanged, otherwise a
    // fetch fires and StoreCached rewrites fetchedAt to the clock instant. The
    // fetch count and persisted stamp are the observable freshness signal.
    private static (int Calls, string? FetchedAt) LoadAt(string dir, long nowMs)
    {
        var stub = new RecordingFetch(SourcePayload());
        using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = new[] { PricingSource.OpenRouter(SourceUrl) },
            CacheDir = dir,
            Fetch = stub.Fetch,
            Clock = () => DateTimeOffset.FromUnixTimeMilliseconds(nowMs),
            TtlMs = 0,
        });
        // touch the handle so the lifecycle load actually runs
        _ = pricing.Catalogs();
        return (stub.Calls, PersistedFetchedAt(dir));
    }

    [Theory]
    [MemberData(nameof(ReadCases))]
    public void ReadDrivesProductionCachePath(string name, string stamp, long? epochMs)
    {
        string dir = TempDir();
        try
        {
            WriteCacheBody(dir, stamp);
            if (epochMs is null)
            {
                // a garbage stamp invalidates the cache entry (treated as absent),
                // so LoadCached rejects it and a fetch is forced
                (int calls, string? fetchedAt) = LoadAt(dir, 0);
                Assert.True(calls == 1, $"timestamps read {name}: garbage must force a fetch");
                Assert.NotEqual(stamp, fetchedAt);
                return;
            }

            long ms = epochMs.Value;
            // at now == epoch the cache age is 0, not > ttl 0, so it is fresh: no
            // fetch, persisted fetchedAt unchanged. This pins the parsed epoch.
            (int freshCalls, string? freshAt) = LoadAt(dir, ms);
            Assert.True(freshCalls == 0, $"timestamps read {name}: at now==epoch must be fresh (no fetch)");
            Assert.Equal(stamp, freshAt);

            // at now == epoch + 1 the age is 1 > ttl 0, so it is stale: a fetch
            // fires and StoreCached rewrites fetchedAt to the clock instant.
            WriteCacheBody(dir, stamp);
            (int staleCalls, string? staleAt) = LoadAt(dir, ms + 1);
            Assert.True(staleCalls == 1, $"timestamps read {name}: at now==epoch+1 must be stale (fetch)");
            Assert.NotEqual(stamp, staleAt);
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Theory]
    [MemberData(nameof(WriteCases))]
    public void WriteDrivesProductionCachePath(long ms)
    {
        string dir = TempDir();
        try
        {
            // a cold fetch at the fixed clock instant runs StoreCached, persisting
            // the fetchedAt this facade writes for that epoch
            (int calls, string? fetchedAt) = LoadAt(dir, ms);
            Assert.True(calls == 1, $"timestamps write {ms}: cold fetch must run StoreCached");
            Assert.False(fetchedAt is null, $"timestamps write {ms}: cache file must exist");

            // assert the real persisted shape: 24 chars, Z-suffixed, integral ms
            Assert.Equal(24, fetchedAt!.Length);
            Assert.EndsWith("Z", fetchedAt);

            // load it back through the production read path: fresh at now == epoch,
            // stale one ms later, which pins the written stamp's epoch round-trip
            (int freshCalls, string? freshAt) = LoadAt(dir, ms);
            Assert.True(freshCalls == 0, $"timestamps write {ms}: reload at now==epoch must be fresh");
            Assert.Equal(fetchedAt, freshAt);

            (int staleCalls, _) = LoadAt(dir, ms + 1);
            Assert.True(staleCalls == 1, $"timestamps write {ms}: reload at now==epoch+1 must be stale");
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }
}

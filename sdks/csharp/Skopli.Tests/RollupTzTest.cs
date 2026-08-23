using System.Text.Json;
using Skopli;
using Xunit;

namespace Skopli.Tests;

/// <summary>
/// Timezone day-bucketing conformance, driven by the SHARED fixtures in
/// <c>golden/rollup-tz/</c>. Each case rolls one synthetic event per timestamp
/// up by day in the case's zone and asserts the resulting buckets equal the
/// shared gold, exercising the same native core the Rust and other-language
/// suites use.
/// </summary>
public sealed class RollupTzTest
{
    public sealed record Bucket(string Key, long Events);

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

    public static IEnumerable<object[]> Cases()
    {
        string path = Path.Combine(RepoRoot(), "golden", "rollup-tz", "cases.json");
        using JsonDocument doc = JsonDocument.Parse(File.ReadAllText(path));
        foreach (JsonElement kase in doc.RootElement.EnumerateArray())
        {
            string name = kase.GetProperty("name").GetString()!;
            string tz = kase.GetProperty("tz").GetString()!;
            var timestamps = kase.GetProperty("timestamps").EnumerateArray()
                .Select(t => t.GetString()!).ToArray();
            var expected = kase.GetProperty("expected").EnumerateArray()
                .Select(b => new Bucket(b.GetProperty("key").GetString()!, b.GetProperty("events").GetInt64()))
                .ToArray();
            yield return new object[] { name, tz, timestamps, expected };
        }
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void BucketsMatchGold(string name, string tz, string[] timestamps, Bucket[] expected)
    {
        var events = timestamps.Select(ts => new UsageEvent
        {
            Harness = "h",
            Timestamp = ts,
            SessionId = "s",
            MessageId = "m",
            Model = "m",
            Tokens = new TokenCounts(),
        }).ToList();

        IReadOnlyList<Rollup> rollups = SkopliClient.Rollup(events, new RollupOptions { By = RollupBy.Day, Tz = tz });

        var got = rollups.Select(r => new Bucket(r.Key, r.Events)).ToArray();
        Assert.True(expected.SequenceEqual(got), $"rollup-tz: {name}");
    }
}

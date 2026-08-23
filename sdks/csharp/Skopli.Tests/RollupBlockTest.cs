using System.Text.Json;
using Skopli;
using Xunit;

namespace Skopli.Tests;

/// <summary>
/// Billing-block windowing conformance, driven by the SHARED fixtures in
/// <c>golden/rollup-block/</c>. Each case rolls one synthetic event per timestamp
/// into blocks of the case's width in the case's zone and asserts the resulting
/// blocks equal the shared gold, exercising the same native core the Rust and
/// other-language suites use.
/// </summary>
public sealed class RollupBlockTest
{
    public sealed record Block(string Key, long Events);

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
        string path = Path.Combine(RepoRoot(), "golden", "rollup-block", "cases.json");
        using JsonDocument doc = JsonDocument.Parse(File.ReadAllText(path));
        foreach (JsonElement kase in doc.RootElement.EnumerateArray())
        {
            string name = kase.GetProperty("name").GetString()!;
            string tz = kase.GetProperty("tz").GetString()!;
            long blockMs = kase.GetProperty("blockMs").GetInt64();
            var timestamps = kase.GetProperty("timestamps").EnumerateArray()
                .Select(t => t.GetString()!).ToArray();
            var expected = kase.GetProperty("expected").EnumerateArray()
                .Select(b => new Block(b.GetProperty("key").GetString()!, b.GetProperty("events").GetInt64()))
                .ToArray();
            yield return new object[] { name, tz, blockMs, timestamps, expected };
        }
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void BlocksMatchGold(string name, string tz, long blockMs, string[] timestamps, Block[] expected)
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

        IReadOnlyList<Rollup> rollups = SkopliClient.Rollup(
            events, new RollupOptions { By = RollupBy.Block, Tz = tz, BlockMs = blockMs });

        var got = rollups.Select(r => new Block(r.Key, r.Events)).ToArray();
        Assert.True(expected.SequenceEqual(got), $"rollup-block: {name}");
    }

    /// <summary>
    /// Omitting <see cref="RollupOptions.BlockMs"/> (leaving it null, which the
    /// wire builder drops) must apply the five-hour default: two events 3h41m
    /// apart join one block anchored to 09:00, and an event past five hours
    /// opens a new block. Guards the facade's absent-optional serialization.
    /// </summary>
    [Fact]
    public void OmittedWidthUsesDefault()
    {
        static List<UsageEvent> Events(params string[] timestamps) =>
            timestamps.Select(ts => new UsageEvent
            {
                Harness = "h",
                Timestamp = ts,
                SessionId = "s",
                MessageId = "m",
                Model = "m",
                Tokens = new TokenCounts(),
            }).ToList();

        IReadOnlyList<Rollup> joined = SkopliClient.Rollup(
            Events("2026-01-01T09:17:00.000Z", "2026-01-01T13:00:00.000Z"),
            new RollupOptions { By = RollupBy.Block, Tz = "UTC" });
        Assert.True(
            new[] { new Block("2026-01-01T09:00:00.000Z", 2) }
                .SequenceEqual(joined.Select(r => new Block(r.Key, r.Events))),
            "omitted-width join");

        IReadOnlyList<Rollup> split = SkopliClient.Rollup(
            Events("2026-01-01T09:00:00.000Z", "2026-01-01T14:30:00.000Z"),
            new RollupOptions { By = RollupBy.Block, Tz = "UTC" });
        Assert.True(
            new[]
            {
                new Block("2026-01-01T09:00:00.000Z", 1),
                new Block("2026-01-01T14:00:00.000Z", 1),
            }.SequenceEqual(split.Select(r => new Block(r.Key, r.Events))),
            "omitted-width split");
    }
}

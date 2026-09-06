using System.Text.Json;
using System.Text.Json.Nodes;
using Skopli;
using Skopli.Internal;
using Xunit;

namespace Skopli.Tests;

/// <summary>
/// Drives the C# probe (<c>Probe.Usable</c>, the same internal function the
/// source lifecycle uses) against the SHARED probe-contract fixtures in
/// <c>golden/pricing/probe/</c>. A facade that drifts from the shared
/// usable/unusable gold fails here.
/// </summary>
public sealed class ProbeTest
{
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
        string path = Path.Combine(RepoRoot(), "golden", "pricing", "probe", "cases.json");
        JsonArray cases = JsonNode.Parse(File.ReadAllText(path))!.AsArray();
        foreach (JsonNode? node in cases)
        {
            JsonObject c = node!.AsObject();
            string name = (string)c["name"]!;
            string format = (string)c["format"]!;
            bool usable = (bool)c["usable"]!;
            // an invalid-JSON payloadRaw is fed as the raw payload text verbatim
            string payload = c.ContainsKey("payloadRaw")
                ? (string)c["payloadRaw"]!
                : c["payload"]!.ToJsonString();
            yield return new object[] { name, format, payload, usable };
        }
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void ProbeConformance(string name, string format, string payload, bool usable)
    {
        var catalog = new Catalog { Source = "probe", Format = format, Payload = payload };
        Assert.True(Probe.Usable("probe", catalog) == usable, name);
    }
}

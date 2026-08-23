using System.Text.Json;
using System.Text.Json.Nodes;

namespace Skopli.Internal;

/// <summary>
/// Central System.Text.Json configuration and the hand-written coercions for the
/// two shapes that are not plain records: the options objects sent down the FFI,
/// and the polymorphic <c>pricing</c> field of a priced group (a hit/miss union
/// keyed on <c>priced</c>). The camelCase wire is matched by explicit
/// <c>JsonPropertyName</c> attributes on the record types, so the serializer needs
/// no naming policy.
/// </summary>
internal static class Json
{
    internal static readonly JsonSerializerOptions Options = new()
    {
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
    };

    internal static T Deserialize<T>(string json)
    {
        T? value = JsonSerializer.Deserialize<T>(json, Options);
        return value ?? throw new InternalException($"native returned null for {typeof(T).Name}");
    }

    internal static List<T> DeserializeArray<T>(string json) =>
        JsonSerializer.Deserialize<List<T>>(json, Options)
        ?? throw new InternalException($"native returned null array for {typeof(T).Name}");

    internal static string Serialize(JsonNode node) => node.ToJsonString(Options);

    // -- enum <-> wire string ------------------------------------------------

    internal static string RollupByWire(RollupBy by) => by switch
    {
        RollupBy.Model => "model",
        RollupBy.Day => "day",
        RollupBy.Session => "session",
        RollupBy.Harness => "harness",
        RollupBy.Workspace => "workspace",
        RollupBy.Block => "block",
        _ => throw new InvalidArgumentException($"unknown rollup dimension: {by}"),
    };

    internal static string PricingModeWire(PricingMode mode) => mode switch
    {
        PricingMode.Calculate => "calculate",
        PricingMode.Auto => "auto",
        PricingMode.Display => "display",
        _ => throw new InvalidArgumentException($"unknown pricing mode: {mode}"),
    };

    // -- priced-group pricing union -----------------------------------------

    /// <summary>
    /// Parse a <c>PricedEventGroup[]</c> JSON array into <see cref="PricedRollup"/>s.
    /// Each element is a rollup bucket with an extra <c>pricing</c> object that is a
    /// hit or a miss depending on its <c>priced</c> flag.
    /// </summary>
    internal static List<PricedRollup> ParsePricedRollups(string json)
    {
        JsonNode root = JsonNode.Parse(json)
            ?? throw new InternalException("native returned null priced rollups");
        var array = root.AsArray();
        var result = new List<PricedRollup>(array.Count);
        foreach (JsonNode? element in array)
        {
            JsonObject obj = element?.AsObject()
                ?? throw new InternalException("priced group is not an object");
            JsonObject pricingObj = obj["pricing"]?.AsObject()
                ?? throw new InternalException("priced group missing pricing");

            // The rollup bucket is the group minus the pricing field.
            var bucketNode = (JsonObject)obj.DeepClone();
            bucketNode.Remove("pricing");
            var rollup = bucketNode.Deserialize<Rollup>(Options)
                ?? throw new InternalException("could not parse rollup bucket");

            PriceLookup lookup = ParseLookup(pricingObj);
            result.Add(new PricedRollup { Rollup = rollup, Pricing = lookup });
        }
        return result;
    }

    /// <summary>Parse a single <c>PriceLookup</c> JSON object (a hit or a miss).</summary>
    internal static PriceLookup ParseLookup(string json)
    {
        JsonObject obj = JsonNode.Parse(json)?.AsObject()
            ?? throw new InternalException("native returned null lookup");
        return ParseLookup(obj);
    }

    private static PriceLookup ParseLookup(JsonObject obj)
    {
        bool priced = obj["priced"]?.GetValue<bool>() ?? false;
        if (priced)
        {
            return new PriceHit
            {
                Model = obj["model"]?.GetValue<string>() ?? string.Empty,
                Key = obj["key"]?.GetValue<string>(),
                Price = obj["price"]?.Deserialize<ModelPrice>(Options),
                Source = obj["source"]?.GetValue<string>(),
                FetchedAt = obj["fetchedAt"]?.GetValue<string?>(),
                Usd = obj["usd"]?.GetValue<double>() ?? 0.0,
                TieredAggregate = obj["tieredAggregate"]?.GetValue<bool>() ?? false,
                Models = obj["models"]?.AsArray().Select(n => n!.GetValue<string>()).ToList(),
            };
        }

        return new PriceMiss
        {
            Model = obj["model"]?.GetValue<string>() ?? string.Empty,
            Attempted = obj["attempted"]?.AsArray().Select(n => n!.GetValue<string>()).ToList()
                        ?? new List<string>(),
            Reason = obj["reason"]?.GetValue<string>(),
            Key = obj["key"]?.GetValue<string?>(),
            Usd = obj["usd"]?.GetValue<double>(),
        };
    }
}

using System.Text.Json.Nodes;

namespace Skopli.Internal;

/// <summary>
/// Builds the options-JSON payloads the FFI expects from the facade's option
/// records. Each option object maps to the camelCase keys the C ABI parses
/// (see the capi crate's json.rs / pricing_opts.rs); absent/null options are
/// omitted so the core applies its defaults.
/// </summary>
internal static class WireBuilder
{
    /// <summary>Serialize a <c>{home?, env?}</c> path options object, or null for no options.</summary>
    internal static string? PathOptions(PathOptions? options)
    {
        if (options is null)
        {
            return null;
        }
        JsonObject obj = PathNode(options);
        return obj.Count == 0 ? null : Json.Serialize(obj);
    }

    /// <summary>Serialize the full <c>ReadUsage</c> options object.</summary>
    internal static string? ReadUsage(ReadUsageOptions options)
    {
        JsonObject obj = PathNode(options);
        if (options.Harnesses is not null)
        {
            var arr = new JsonArray();
            foreach (string h in options.Harnesses)
            {
                arr.Add(h);
            }
            obj["harnesses"] = arr;
        }
        if (options.Since is not null)
        {
            obj["since"] = options.Since;
        }
        if (options.Until is not null)
        {
            obj["until"] = options.Until;
        }
        if (options.Tz is not null)
        {
            obj["tz"] = options.Tz;
        }
        if (options.Subagents == SubagentMode.Exclude)
        {
            obj["subagents"] = "exclude";
        }
        return obj.Count == 0 ? null : Json.Serialize(obj);
    }

    /// <summary>Serialize a rollup / price-events <c>{by, tz?, blockMs?}</c> options object.</summary>
    internal static string RollupOptions(RollupOptions options)
    {
        var obj = new JsonObject { ["by"] = Json.RollupByWire(options.By) };
        if (options.Tz is not null)
        {
            obj["tz"] = options.Tz;
        }
        if (options.BlockMs is long blockMs)
        {
            obj["blockMs"] = blockMs;
        }
        return Json.Serialize(obj);
    }

    /// <summary>Serialize a <c>ModelPrice</c> record for <c>ag_cost_usd</c>.</summary>
    internal static string ModelPrice(ModelPrice price)
    {
        var obj = new JsonObject
        {
            ["input"] = price.Input,
            ["output"] = price.Output,
        };
        AddOptional(obj, "cacheRead", price.CacheRead);
        AddOptional(obj, "cacheWrite", price.CacheWrite);
        AddOptional(obj, "cacheWrite1h", price.CacheWrite1h);
        if (price.Tiers is { Count: > 0 })
        {
            var tiers = new JsonArray();
            foreach (PriceTier t in price.Tiers)
            {
                var tier = new JsonObject
                {
                    ["threshold"] = t.Threshold,
                    ["input"] = t.Input,
                    ["output"] = t.Output,
                };
                AddOptional(tier, "cacheRead", t.CacheRead);
                AddOptional(tier, "cacheWrite", t.CacheWrite);
                AddOptional(tier, "cacheWrite1h", t.CacheWrite1h);
                tiers.Add(tier);
            }
            obj["tiers"] = tiers;
        }
        return Json.Serialize(obj);
    }

    /// <summary>Serialize a <c>TokenCounts</c> record for <c>ag_cost_usd</c>.</summary>
    internal static string TokenCounts(TokenCounts tokens)
    {
        var obj = new JsonObject
        {
            ["input"] = tokens.Input,
            ["output"] = tokens.Output,
            ["cacheRead"] = tokens.CacheRead,
            ["cacheWrite"] = tokens.CacheWrite,
            ["reasoning"] = tokens.Reasoning,
        };
        if (tokens.CacheWrite1h is { } cw)
        {
            obj["cacheWrite1h"] = cw;
        }
        return Json.Serialize(obj);
    }

    /// <summary>Serialize a <c>UsageEvent[]</c> array via the record's own attributes.</summary>
    internal static string Events(IEnumerable<UsageEvent> events) =>
        System.Text.Json.JsonSerializer.Serialize(events, Json.Options);

    /// <summary>Serialize a <c>Rollup[]</c> array via the record's own attributes.</summary>
    internal static string Rollups(IEnumerable<Rollup> rollups) =>
        System.Text.Json.JsonSerializer.Serialize(rollups, Json.Options);

    /// <summary>
    /// Serialize the <c>ag_pricing_new</c> options from mode + overrides + the
    /// merged catalogs (facade-side sources already resolved). Always sends
    /// <c>builtinSources: false</c> - the ABI never fetches.
    /// </summary>
    internal static string Pricing(PricingMode mode, IReadOnlyList<PriceOverride>? overrides, IReadOnlyList<Catalog> catalogs)
    {
        var obj = new JsonObject
        {
            ["mode"] = Json.PricingModeWire(mode),
            ["builtinSources"] = false,
        };
        if (overrides is { Count: > 0 })
        {
            var arr = new JsonArray();
            foreach (PriceOverride o in overrides)
            {
                var entry = new JsonObject
                {
                    ["model"] = o.Model,
                    ["input"] = o.Input,
                    ["output"] = o.Output,
                };
                AddOptional(entry, "cacheRead", o.CacheRead);
                AddOptional(entry, "cacheWrite", o.CacheWrite);
                AddOptional(entry, "cacheWrite1h", o.CacheWrite1h);
                arr.Add(entry);
            }
            obj["overrides"] = arr;
        }
        if (catalogs.Count > 0)
        {
            var arr = new JsonArray();
            foreach (Catalog c in catalogs)
            {
                arr.Add(CatalogNode(c));
            }
            obj["catalogs"] = arr;
        }
        return Json.Serialize(obj);
    }

    private static JsonNode CatalogNode(Catalog c)
    {
        var obj = new JsonObject { ["source"] = c.Source };
        if (c.FetchedAt is not null)
        {
            obj["fetchedAt"] = c.FetchedAt;
        }
        if (c.Format is not null)
        {
            obj["format"] = c.Format;
            if (c.Payload is null)
            {
                throw new InvalidArgumentException($"catalog '{c.Source}' has a format but no payload");
            }
            obj["payload"] = JsonNode.Parse(c.Payload)
                ?? throw new InvalidArgumentException($"catalog '{c.Source}' payload is not valid JSON");
        }
        else if (c.Prices is not null)
        {
            var prices = new JsonObject();
            foreach ((string model, ModelPrice price) in c.Prices)
            {
                prices[model] = JsonNode.Parse(ModelPrice(price));
            }
            obj["prices"] = prices;
        }
        else
        {
            throw new InvalidArgumentException($"catalog '{c.Source}' needs either prices or format+payload");
        }
        return obj;
    }

    private static JsonObject PathNode(PathOptions options)
    {
        var obj = new JsonObject();
        if (options.Home is not null)
        {
            obj["home"] = options.Home;
        }
        if (options.Env is not null)
        {
            var env = new JsonObject();
            foreach ((string k, string v) in options.Env)
            {
                env[k] = v;
            }
            obj["env"] = env;
        }
        return obj;
    }

    private static void AddOptional(JsonObject obj, string key, double? value)
    {
        if (value is { } v)
        {
            obj[key] = v;
        }
    }
}

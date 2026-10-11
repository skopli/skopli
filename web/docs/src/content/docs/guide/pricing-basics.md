---
title: Pricing basics
description: Where prices come from, what the Price layer does on the network and when, how to run it from cache or with no sources at all, and what a priced row looks like.
---

## Where prices come from

`createPricing()` with no options loads three public catalogs in priority order: OpenRouter, LiteLLM and models.dev.
These are market listings of per-million-token rates.
They are not your invoice. A negotiated rate, a subscription or a free tier will differ, and Skopli's result is the list price for the tokens it saw.

## Network behaviour

Read and Rollup never use the network.
Price does, by default:

- On first use, each source is fetched with a 10-second timeout and written to the cache directory.
- The cache is reused for one hour (`ttlMs`), then refreshed on the next call.
- A fetch that fails falls back to the cached copy, stale or not. With no cache either, that source is skipped for this run and `catalogs()` shows what loaded.

Cache location: `$XDG_CACHE_HOME/skopli` or `~/.cache/skopli` on Linux, `~/Library/Caches/skopli` on macOS, `%LOCALAPPDATA%\skopli\cache` on Windows.
`defaultCacheDir()` returns it.

### Running without the network

```ts tab=offline
// Use the cache, never fetch. Misses if a source was never cached.
const cached = createPricing({ offline: true });

// No market sources at all: price from your own table only.
const fixed = createPricing({
  sources: [],
  overrides: [
    { model: "claude-sonnet-4.5", input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 },
  ],
});
```

```python tab=offline
cached = skopli.create_pricing(offline=True)
fixed = skopli.create_pricing(sources=[], overrides=[{"model": "claude-sonnet-4.5", "input": 3, "output": 15}])
```

```ruby tab=offline
cached = Skopli.create_pricing(offline: true)
fixed = Skopli.create_pricing(sources: [], overrides: [{ model: "claude-sonnet-4.5", input: 3, output: 15 }])
```

```go tab=offline
cached, _ := skopli.NewPricing(skopli.PricingOptions{Offline: true})
```

```java tab=offline
var cached = Skopli.createPricing(PricingOptions.builder().offline(true).build());
```

```csharp tab=offline
using var cached = SkopliClient.CreatePricing(new PricingOptions { Offline = true });
```

```swift tab=offline
let cached = try Skopli.createPricing(options: PricingOptions(offline: true))
```

```c tab=offline
const char *opts = "{\"mode\":\"calculate\",\"builtinSources\":false,\"catalogs\":[...]}";
AgPricing *p = NULL;
AgStatus st = ag_pricing_new(opts, strlen(opts), &p);
```

Overrides are consulted before any source, so an override also pins a price for a model the catalogs list differently.

The default `libskopli` build has no built-in sources: pass `"builtinSources":false` and your own `catalogs`, or `ag_pricing_new` returns `AG_STATUS_CATALOG`.
`libskopli-full` (built with the `net` feature) fetches the three default catalogs unless you set `builtinSources` to false.

## Pricing a rollup

```ts tab=price
const pricing = createPricing();
const rows = await pricing.priceRollups(rollup(events, { by: "model" }));

for (const row of rows) {
  if (row.pricing.priced) {
    console.log(row.key, row.pricing.usd.toFixed(2), row.pricing.source, row.pricing.key);
  } else {
    console.log(row.key, "no price; tried", row.pricing.attempted.join(", "));
  }
}
```

```python tab=price
rows = pricing.price_rollups(skopli.rollup(result.events, by="model"))
for row in rows:
    if row.pricing.priced:
        print(row.rollup.key, round(row.usd, 2), row.pricing.source)
    else:
        print(row.rollup.key, "no price; tried", ", ".join(row.pricing.attempted))
```

```ruby tab=price
pricing.price_rollups(Skopli.rollup(result.events, :model)).each do |row|
  puts row.pricing.priced ? "#{row.rollup.key} #{row.usd.round(2)}" : "#{row.rollup.key} no price"
end
```

```go tab=price
rows, err := pricing.PriceRollups(byModel)
```

```java tab=price
var rows = pricing.priceRollups(byModel);
```

```csharp tab=price
var rows = pricing.PriceRollups(byModel);
```

```swift tab=price
let rows = try pricing.priceRollups(rollups: byModel)
```

```c tab=price
AgBuf rows;
AgStatus st = ag_pricing_price_rollups(p, (const char *)rollups.ptr, rollups.len, &rows);
```

A hit tells you the matched catalog `key`, the `source` it came from, when that catalog was `fetchedAt`, the `price` applied and the `usd` result.
A miss tells you which names were `attempted`.
Nothing is estimated for a miss.

## Modes

Some logs record a cost next to each message.
`mode` decides how that recorded figure and the catalog figure combine.

| `mode`                | Hit `usd`                                                   | Miss `usd`                     |
| --------------------- | ----------------------------------------------------------- | ------------------------------ |
| `calculate` (default) | Catalog price applied to the tokens                         | absent                         |
| `display`             | The recorded cost, or 0 when none was logged                | The recorded cost, or 0        |
| `auto`                | The recorded cost when present, otherwise the catalog price | The recorded cost when present |

Use `calculate` to compare agents on one price list.
Use `display` to reproduce what the agent itself showed you.
Use `auto` when you trust the log's figure and want the catalog only as a fallback.

## Next

- [Model matching](/skopli/pricing/model-matching/): how a log's model name finds a catalog key.
- [Cost math](/skopli/pricing/cost-math/): the formula, cache rates and defaults.
- [Rollups vs events](/skopli/pricing/rollups-vs-events/): when to use `priceEvents` instead.

---
title: Rollups vs events
description: priceRollups sums first and prices second; priceEvents prices each event and then groups. They agree for flat prices and can differ for tiered ones.
---

## Two orders of operation

`priceRollups(rollups)` takes rows that already contain summed tokens and applies one price per row.
`priceEvents(events, { by })` prices each event on its own and then groups the results the way `rollup` would.

For a model with a flat price, both give the same number.
For a model with long-context tiers, they can differ, because tier selection depends on the context size of each request, and a sum has no request size.

## Which to use

| Situation                                                     | Call                                    |
| ------------------------------------------------------------- | --------------------------------------- |
| Flat-priced models, or you already have rollups               | `priceRollups`                          |
| Any model with tiers, or you want a per-request-correct total | `priceEvents`                           |
| A quick look where a flagged row is acceptable                | `priceRollups`, check `tieredAggregate` |

`priceEvents` is never less correct.
`priceRollups` exists because rollups are what you usually hold, and it flags the rows where the shortcut mattered.

## priceEvents

```ts tab=events
const pricing = createPricing();
const groups = await pricing.priceEvents(events, { by: "day", tz: "UTC" });

for (const g of groups) {
  if (!g.pricing.priced) console.log(g.key, "unpriced:", g.pricing.model);
  else if ("models" in g.pricing) console.log(g.key, g.pricing.usd, "across", g.pricing.models);
  else console.log(g.key, g.pricing.usd, g.pricing.source);
}
```

```python tab=events
groups = pricing.price_events(result.events, by="day", tz="UTC")
```

```ruby tab=events
groups = pricing.price_events(result.events, :day, tz: "UTC")
```

```go tab=events
groups, err := pricing.PriceEvents(result.Events, skopli.RollupOptions{By: skopli.ByDay, TZ: "UTC"})
```

```java tab=events
var groups = pricing.priceEvents(result.events(), RollupOptions.by(RollupBy.DAY, "UTC"));
```

```csharp tab=events
var groups = pricing.PriceEvents(result.Events, new RollupOptions { By = RollupBy.Day, Tz = "UTC" });
```

```swift tab=events
let groups = try pricing.priceEvents(events: result.events, by: .day, tz: "UTC")
```

```c tab=events
const char *opts = "{\"by\":\"day\",\"tz\":\"UTC\"}";
AgBuf groups;
AgStatus st = ag_pricing_price_events(p, (const char *)events.ptr, events.len, opts, strlen(opts), &groups);
```

## The group result

A group's `pricing` is one of three shapes:

- A hit, when every event in the group was priced and they all share one model. It carries the usual `key`, `source`, `fetchedAt` and the summed `usd`.
- An aggregate hit, `{ priced: true, usd, models: [...] }`, when every event was priced but the group spans several models (a day, a session). There is no single catalog key to report, so the distinct model list is given instead.
- A miss, when at least one event could not be priced. `attempted` lists the candidates for the unpriced model; `usd` is present only in `display` or `auto` mode when the log recorded costs.

A group is a miss as soon as one event is unpriced.
Partial sums are not reported as totals.

## Both calls, one catalog load

`priceRollups` and `priceEvents` share the pricing object's catalogs, so calling both costs one fetch (or none, from cache).

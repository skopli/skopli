---
title: Rollups
description: Group events by model, day, session, harness, workspace or 5-hour block, and understand what the summed counters mean.
---

## One call, six dimensions

```ts
type RollupBy = "model" | "day" | "session" | "harness" | "workspace" | "block";

type RollupOptions = {
  by: RollupBy;
  tz?: string; // for "day" and "block"; default: system zone
  blockMs?: number; // for "block"; default 18,000,000 (5 hours)
};
```

```ts tab=rollup
import { rollup } from "skopli";

const byDay = rollup(events, { by: "day", tz: "UTC" });
const byModel = rollup(events, { by: "model" });
```

```python tab=rollup
by_day = skopli.rollup(result.events, by="day", tz="UTC")
by_model = skopli.rollup(result.events, by="model")
```

```ruby tab=rollup
by_day = Skopli.rollup(result.events, :day, tz: "UTC")
by_model = Skopli.rollup(result.events, :model)
```

```go tab=rollup
byDay, _ := skopli.Rollup(result.Events, skopli.RollupOptions{By: skopli.ByDay, TZ: "UTC"})
```

```java tab=rollup
var byDay = Skopli.rollup(result.events(), RollupOptions.by(RollupBy.DAY, "UTC"));
```

```csharp tab=rollup
var byDay = SkopliClient.Rollup(result.Events, new RollupOptions { By = RollupBy.Day, Tz = "UTC" });
```

```swift tab=rollup
let byDay = try Skopli.rollup(events: result.events, by: .day, tz: "UTC")
```

```c tab=rollup
const char *opts = "{\"by\":\"day\",\"tz\":\"UTC\"}";
AgBuf by_day;
AgStatus st = ag_rollup((const char *)events.ptr, events.len, opts, strlen(opts), &by_day);
```

## The row

```ts
type Rollup = {
  key: string; // the group: model name, "2026-08-01", session id, ...
  tokens: TokenCounts; // summed per counter
  events: number;
  turns: number;
  calls: number; // each event contributes its reported count, else 1
  costUsd?: number; // sum over the events that reported one
};
```

Rows come back sorted by key.
Token counters are summed per counter, so they remain disjoint after aggregation.

## Dimensions

| `by`        | Key                              | Notes                                                                                                                                                                                                                                              |
| ----------- | -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `model`     | model name as logged             | The same model can appear under several names across harnesses; pricing normalizes them, rollup does not                                                                                                                                           |
| `day`       | `YYYY-MM-DD` in `tz`             | Day boundaries follow the zone you pass, not the machine's                                                                                                                                                                                         |
| `session`   | `sessionId`                      | Session ids are only unique within a harness                                                                                                                                                                                                       |
| `harness`   | harness id                       | Useful for "which agent is spending"                                                                                                                                                                                                               |
| `workspace` | `workspace`                      | Events without a workspace group under the key `(unknown)`                                                                                                                                                                                         |
| `block`     | ISO timestamp of the block start | A block opens at the first event's timestamp floored to the hour in `tz` and lasts `blockMs` (5 hours by default, matching common subscription quota windows); the next block opens at the first event after it ends, so there are no empty blocks |

## `costUsd` on a rollup

A rollup's `costUsd` is the sum over the events in the group that reported a cost; events without one contribute nothing, and the field is absent only when no event carried a cost.
The figure can therefore be partial: a group of 50 events where 10 report a cost gives you the sum of those 10, with nothing to say the other 40 are missing.
Price the rollup to get a complete figure, or compare `events` with the per-harness `costUsd` column on the harness page to see which logs record a cost at all.
To get a cost for every row, price the rollup; see [Pricing basics](/skopli/guide/pricing-basics/).

## Rollup first or price first?

Rollup sums tokens before pricing sees them.
For models whose price changes above a context threshold, that can move a group across the threshold and change the cost.
[Rollups vs events](/skopli/pricing/rollups-vs-events/) explains when this matters and how `priceEvents` avoids it.

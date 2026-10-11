---
title: What Skopli reads
description: Skopli is an SDK that reads the session logs your AI coding agents already keep on disk, turns them into usage events, aggregates them, and prices them.
---

## The data is already on your disk

Every coding agent writes a session log while it works: Claude Code, Codex CLI, Gemini CLI, OpenCode and 36 more.
Those logs record which model answered, how many tokens went in and out, what was served from cache, and when.
Skopli reads those files and nothing else.
It does not sit between you and the model, does not install a proxy, and does not need an account or an API key.

## Three layers

| Layer  | What it does                                                                                         | Network                                |
| ------ | ---------------------------------------------------------------------------------------------------- | -------------------------------------- |
| Read   | Finds each harness's log directory, parses the files, emits one `UsageEvent` per assistant record    | None                                   |
| Rollup | Groups events by model, day, session, harness, workspace or 5-hour block and sums the token counters | None                                   |
| Price  | Matches each model name against market catalogs and multiplies tokens by the matched rates           | Fetches catalogs by default; see below |

Read and Rollup never open a socket.
Price fetches three public catalogs (OpenRouter, LiteLLM, models.dev) the first time you call it, caches them under your user cache directory for an hour, and works from that cache afterwards.
Pass `sources: []` to price from overrides alone, or `offline: true` to use whatever is cached and never fetch.
[Pricing basics](/skopli/guide/pricing-basics/) covers each option.

## Four calls

Detect which harnesses are present, read their events, roll them up, price the result.
The Rust core reads and aggregates; the facades fetch catalogs and hand them to the core, so in Rust you pass catalogs in yourself, and the C library behaves the same way.

```ts tab=quickstart
import { detectHarnesses, readUsage, rollup, createPricing } from "skopli";

const detection = detectHarnesses();
const { events, diagnostics } = await readUsage({ since: "2026-08-01" });
const byModel = rollup(events, { by: "model" });
const priced = await createPricing().priceRollups(byModel);
```

```python tab=quickstart
import skopli

detection = skopli.detect_harnesses()
result = skopli.read_usage(since="2026-08-01")
by_model = skopli.rollup(result.events, by="model")
priced = skopli.create_pricing().price_rollups(by_model)
```

```ruby tab=quickstart
require "skopli"

detection = Skopli.detect_harnesses
result = Skopli.read_usage(since: "2026-08-01")
by_model = Skopli.rollup(result.events, :model)
priced = Skopli.create_pricing.price_rollups(by_model)
```

```go tab=quickstart
detection, _ := skopli.DetectHarnesses(nil)
result, _ := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01"})
byModel, _ := skopli.Rollup(result.Events, skopli.RollupOptions{By: skopli.ByModel})
pricing, _ := skopli.NewPricing(skopli.PricingOptions{})
defer pricing.Close()
priced, _ := pricing.PriceRollups(byModel)
```

```java tab=quickstart
var detection = Skopli.detectHarnesses(DetectOptions.defaults());
var result = Skopli.readUsage(ReadUsageOptions.builder().since("2026-08-01").build());
var byModel = Skopli.rollup(result.events(), RollupOptions.by(RollupBy.MODEL));
try (var pricing = Skopli.createPricing(PricingOptions.builder().build())) {
  var priced = pricing.priceRollups(byModel);
}
```

```csharp tab=quickstart
var detection = SkopliClient.DetectHarnesses();
var result = SkopliClient.ReadUsage(new ReadUsageOptions { Since = "2026-08-01" });
var byModel = SkopliClient.Rollup(result.Events, new RollupOptions { By = RollupBy.Model });
using var pricing = SkopliClient.CreatePricing(new PricingOptions());
var priced = pricing.PriceRollups(byModel);
```

```swift tab=quickstart
let detection = try Skopli.detectHarnesses()
let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01"))
let byModel = try Skopli.rollup(events: result.events, by: .model)
let pricing = try Skopli.createPricing()
let priced = try pricing.priceRollups(rollups: byModel)
```

```rust tab=quickstart
use skopli_core::pricing::{Pricing, PricingMode};
use skopli_core::readers::{registered_readers, ReaderContext};
use skopli_core::rollup::{rollup, RollupBy, RollupOptions};

let ctx = ReaderContext::new(std::env::var("HOME")?);
let mut events = Vec::new();
for reader in registered_readers() {
    events.extend(reader.read(&ctx).events);
}
let by_model = rollup(&events, &RollupOptions::new(RollupBy::Model));
let priced = Pricing::new(catalogs, None, PricingMode::Calculate).price_rollups(&by_model);
```

```c tab=quickstart
const char *read_opts = "{\"since\":\"2026-08-01\"}";
const char *by_model = "{\"by\":\"model\"}";
AgBuf detection, events, rollups, priced;
AgPricing *pricing = NULL;

ag_detect_harnesses(NULL, 0, &detection);
ag_read_usage(read_opts, strlen(read_opts), &events);
ag_rollup((const char *)events.ptr, events.len, by_model, strlen(by_model), &rollups);
ag_pricing_new(pricing_opts, strlen(pricing_opts), &pricing);
ag_pricing_price_rollups(pricing, (const char *)rollups.ptr, rollups.len, &priced);
```

Every priced row is either a hit, with the matched catalog key, the source it came from and a `usd` figure, or a miss with `priced: false` and the list of model names that were tried.
Skopli never substitutes a guess for a missing price.

## What the counts mean

A rollup row carries three counts that are easy to conflate.

- `events`: normalized records, one per assistant message in most logs.
- `turns`: assistant records that directly follow a user record. A turn is roughly "one answer to one prompt".
- `calls`: underlying model calls. Some logs aggregate a whole session into one record and report how many calls it covered, so `calls` can exceed `events`.

Token counters are disjoint: `input`, `output`, `cacheRead`, `cacheWrite` and `reasoning` never overlap, and `output` excludes reasoning where the source reports reasoning inside output.
[Reading usage](/skopli/guide/reading/) has the full semantics.

## What Skopli is not

- Not a CLI. It is a library with facades in nine languages; you build the report, dashboard or alert.
- Not a proxy or instrumentation layer. It reads logs after the fact.
- Not a price authority. Catalog prices are market listings; your invoice is the truth.
- Not a telemetry service. Nothing you read leaves your machine.
- Not published yet. Build from source until the first release.

## Where next

- [Getting started](/skopli/guide/getting-started/): build the SDK for your language and run the four calls.
- [Supported harnesses](/skopli/reference/harnesses/): the 40 harnesses and what each log provides.
- [Rollups vs events](/skopli/pricing/rollups-vs-events/): when summing before pricing changes the answer.

---
title: API reference
description: Every public function and type, with the TypeScript signatures as the reference form and the facade names for the other eight languages.
---

## Entry points

| TypeScript        | Python              | Ruby                       | Go                | Java                     | C#                             | Swift                    | C ABI                  |
| ----------------- | ------------------- | -------------------------- | ----------------- | ------------------------ | ------------------------------ | ------------------------ | ---------------------- |
| `detectHarnesses` | `detect_harnesses`  | `Skopli.detect_harnesses`  | `DetectHarnesses` | `Skopli.detectHarnesses` | `SkopliClient.DetectHarnesses` | `Skopli.detectHarnesses` | `ag_detect_harnesses`  |
| `readUsage`       | `read_usage`        | `Skopli.read_usage`        | `ReadUsage`       | `Skopli.readUsage`       | `SkopliClient.ReadUsage`       | `Skopli.readUsage`       | `ag_read_usage`        |
| `rollup`          | `rollup`            | `Skopli.rollup`            | `Rollup`          | `Skopli.rollup`          | `SkopliClient.Rollup`          | `Skopli.rollup`          | `ag_rollup`            |
| `costUsd`         | `cost_usd`          | `Skopli.cost_usd`          | `CostUSD`         | `Skopli.costUsd`         | `SkopliClient.CostUsd`         | `Skopli.costUsd`         | `ag_cost_usd`          |
| `createPricing`   | `create_pricing`    | `Skopli.create_pricing`    | `NewPricing`      | `Skopli.createPricing`   | `SkopliClient.CreatePricing`   | `Skopli.createPricing`   | `ag_pricing_new`       |
| `defaultCacheDir` | `default_cache_dir` | `Skopli.default_cache_dir` | `DefaultCacheDir` | `Skopli.defaultCacheDir` | `SkopliClient.DefaultCacheDir` | `Skopli.defaultCacheDir` | `ag_default_cache_dir` |

The Rust crate `skopli-core` is the engine underneath all of these.
It exposes the readers, `rollup`, `cost_usd` and a `Pricing` type that takes already-loaded catalogs; catalog fetching and caching live in each facade.

## detectHarnesses

```ts
function detectHarnesses(options?: PathResolverOptions): Detection;

type Detection = {
  supported: Harness[]; // harnesses with a log directory on this machine
  unsupported: string[]; // detected tools Skopli does not read
};
```

Reads directory listings only.

## readUsage

```ts
function readUsage(options?: ReadUsageOptions): Promise<ReadUsageResult>;

type ReadUsageOptions = PathResolverOptions & {
  harnesses?: Harness[];
  since?: Date | string;
  until?: Date | string;
  tz?: string;
  subagents?: "include" | "exclude";
};

type ReadUsageResult = {
  events: UsageEvent[];
  diagnostics: Diagnostic[];
  skipped: Partial<Record<Harness, string[]>>;
};
```

Field semantics are in [Reading usage](/skopli/guide/reading/).

## rollup

```ts
function rollup(events: UsageEvent[], options: RollupOptions): Rollup[];

type RollupOptions = { by: RollupBy; tz?: string; blockMs?: number };
type RollupBy = "model" | "day" | "session" | "harness" | "workspace" | "block";
```

Pure and synchronous.
Described in [Rollups](/skopli/guide/rollups/).

## costUsd

```ts
function costUsd(tokens: TokenCounts, price: ModelPrice): number;
```

Applies a price to a token count.
The arithmetic, including cache tiers and long-context tiers, is in [Cost math](/skopli/pricing/cost-math/).

## createPricing

```ts
function createPricing(options?: CreatePricingOptions): Pricing;

type CreatePricingOptions = {
  sources?: PricingSource[];
  overrides?: PriceOverride[];
  mode?: PricingMode;
  cacheDir?: string;
  offline?: boolean;
  ttlMs?: number;
  refresh?: boolean;
  fetch?: FetchLike;
};

type Pricing = {
  lookupModel(model: string): Promise<PriceLookup>;
  priceTokens(model: string, tokens: TokenCounts): Promise<PriceLookup & { usd?: number }>;
  priceRollups(rollups: Rollup[]): Promise<PricedRollup[]>;
  priceEvents(
    events: UsageEvent[],
    options: { by: RollupBy; tz?: string },
  ): Promise<PricedEventGroup[]>;
  catalogs(): Promise<CatalogInfo[]>;
};
```

`priceTokens` exists in the TypeScript, Python and Ruby facades; Go, Java, C#, Swift and the C ABI go through `priceRollups` or `priceEvents`.

### Results

```ts
type PriceHit = {
  priced: true;
  model: string;
  key: string; // catalog key that matched
  price: ModelPrice;
  source: string; // "override", "openrouter", "litellm", "models-dev", or a custom source name
  fetchedAt: string | null; // null for overrides
};

type PriceMiss = {
  priced: false;
  model: string;
  attempted: string[];
  reason?: "zeroCost"; // every source ended on a zero-rate placeholder
  key?: string; // the placeholder's key, with reason
};

type PricedRollup = Rollup & {
  pricing: (PriceHit & { usd: number; tieredAggregate?: true }) | (PriceMiss & { usd?: number });
};

type CatalogInfo = { source: string; fetchedAt: string | null; models: number };
```

Whether a miss carries `usd` depends on the mode: `display` always sets it (0 when the rollup logged no `costUsd`), `auto` sets it to the logged `costUsd` when there is one, `calculate` never sets it.

### Prices

```ts
type ModelPrice = {
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
  cacheWrite1h?: number;
  tiers?: PriceTier[];
  tierMode?: "marginal" | "whole-request";
};

type PriceTier = { threshold: number } & Omit<ModelPrice, "tiers" | "tierMode">;
type PriceOverride = ModelPrice & { model: string };
```

### Custom sources

```ts
type PricingSource = {
  name: string;
  load(context: SourceContext): Promise<PricingCatalog | null>;
};

type PricingCatalog = {
  source: string;
  fetchedAt: string | null;
  prices: Map<string, ModelPrice>;
};
```

A source that returns `null` is skipped for that load.
Built-in sources return `null` when the fetch fails and no cache exists, so a fully offline first run prices from overrides alone and reports misses for everything else.

## C ABI

`libskopli` exports 17 functions.
Inputs are UTF-8 JSON passed as a pointer and length; results come back through an `AgBuf *out` parameter (a `ptr`/`len` pair the library owns and `ag_buf_free` releases), and every fallible call returns an `AgStatus` (`AG_STATUS_OK`, `AG_STATUS_INVALID_ARGUMENT`, `AG_STATUS_CATALOG` or `AG_STATUS_INTERNAL`).
The pricing engine is an opaque `AgPricing *` handle from `ag_pricing_new`.

```c
AgBuf out;
if (ag_read_usage(opts_json, strlen(opts_json), &out) != AG_STATUS_OK) {
  char *message = NULL;
  ag_last_error_message(&message);
  fputs(message, stderr);
  ag_string_free(message);
}
ag_buf_free(out);
```

| Function                                                                                                    | Purpose                                                             |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `ag_abi_version`, `ag_schema_version`, `ag_version`                                                         | Compatibility checks before any other call                          |
| `ag_default_cache_dir`                                                                                      | The platform cache path, as `defaultCacheDir`                       |
| `ag_detect_harnesses`, `ag_read_usage`, `ag_rollup`, `ag_cost_usd`                                          | The four stateless operations                                       |
| `ag_pricing_new`, `ag_pricing_free`                                                                         | Build an engine from catalogs JSON and options; release it          |
| `ag_pricing_lookup_model`, `ag_pricing_price_rollups`, `ag_pricing_price_events`, `ag_pricing_catalog_info` | Engine queries                                                      |
| `ag_buf_free`, `ag_string_free`                                                                             | Release a returned buffer or string                                 |
| `ag_last_error_message`                                                                                     | The last error on this thread, when a call returned a non-OK status |

The library opens no socket.
Pass catalogs you fetched yourself, or build `skopli-capi` with `--features net` to enable the built-in sources inside the library.
Go, Java, C# and Swift wrap this ABI, which is why they need `libskopli` on the library path.

## Versions

`ag_schema_version` reports the event schema version, and each facade exposes it as `schemaVersion` so a caller can compare the binding it compiled against with the core it loaded.

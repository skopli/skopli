# Pricing lifecycle conformance fixtures

Shared gold for the pricing-lifecycle conformance matrix: the five base
behaviors plus the three hardening behaviors (`fetched-empty`, `cached-empty`,
`live-reload`).
Every facade's lifecycle test loads these same files so a facade that drifts
fails against shared gold rather than its author's assumptions. The TypeScript
reference (`test/pricing.test.ts`) defines the gold.

## Files

- `source-openrouter.json`, `source-litellm.json`, `source-modelsdev.json` —
  canonical raw source payloads, one per built-in source, in the on-the-wire
  shape each source parser consumes. Each parses through its format and prices
  the shared `openai/gpt-5` rollup to `pricedUsd` (2.25); the reference suite
  asserts this for all three so their parser shapes stay valid.
- `source-openrouter-empty.json` — a syntactically valid OpenRouter payload
  that parses to zero prices (unusable), for the `fetched-empty` behavior.
- `cache-fresh.json`, `cache-stale.json` — canonical cache files
  (`{ fetchedAt, payload }`), each wrapping the OpenRouter source payload. The
  `fetchedAt` timestamps are fixed so age is deterministic relative to `now`.
- `cache-fresh-empty.json` — a fresh-aged cache whose payload parses to zero
  prices, for the `cached-empty` behavior.
- `request.json` — the single rollup every behavior prices, plus the pricing
  options (`mode: "calculate"`) and the clock/TTL knobs the harness applies.
  `nowSecond` is the advanced clock for `live-reload`.
- `expected/<behavior>.json` — the expected outcome per behavior: whether a
  fetch was issued (`fetched`), the resulting cache `fetchedAt`, and the priced
  total (`pricedUsd`). `live-reload.json` instead pins the two-query shape:
  `fetchesTotal`, `fetchedAtFirst`, `fetchedAtSecond`.

## Timestamps and clock

All timestamps are UTC ISO-8601 with millisecond precision.

- `fetchedAtFresh` = `2026-08-01T00:00:00.000Z`
- `fetchedAtStale` = `2020-01-01T00:00:00.000Z`
- `now` (test clock) = `2026-08-01T00:01:00.000Z`
- `fetchedAtFetched` (a live fetch stamps the file with `now`) =
  `2026-08-01T00:01:00.000Z`
- `ttlMs` = `60000`. The fresh cache's age is `now - fetchedAtFresh` =
  `60000` ms, exactly equal to `ttlMs`. The shared cross-facade contract is
  `age > ttlMs` means stale, so `age == ttlMs` is fresh — the fresh fixture sits
  precisely on that boundary (an exact-boundary fixture is the strongest test of
  the comparison). This matches the TypeScript reference (`src/pricing/cache.ts`:
  `stale = age > ttlMs`) and the Rust core (`stale: age > ttl_ms`). The stale
  cache's age (years) far exceeds `ttlMs`.

## The five behaviors

1. `cold-fetch` — empty cacheDir, fetch returns `source-openrouter.json`; the
   file is written with `fetchedAt = now` and the rollup prices to `pricedUsd`.
2. `warm-cache` — `cache-fresh.json` present, fetch must not be called; priced
   from cache, `fetchedAt` unchanged.
3. `ttl-refresh` — `cache-stale.json` present with `refresh: true`; fetch called
   once, `fetchedAt` advances to `now`.
4. `offline` — `offline: true` with `cache-stale.json` present; served from
   cache without fetching, `fetchedAt` unchanged. The `offline-no-cache`
   subcase (`expected/offline-no-cache.json`) covers `offline: true` with an
   empty cacheDir: the source is skipped, no fetch is issued, and the rollup
   degrades to an unpriced miss rather than crashing (`fetched: false`,
   `priced: false`, `fetchedAt: null`).
5. `stale-fallback` — `cache-stale.json` present and fetch throws; the stale
   payload is served, `fetchedAt` unchanged. The fetch is attempted exactly
   once (`fetched: true`) before the fallback.

## The three hardening behaviors

6. `fetched-empty` — `cache-stale.json` present, fetch succeeds but returns
   `source-openrouter-empty.json` (parses to zero prices). An unusable fetched
   payload is treated like a fetch failure: the stale catalog is served, the
   cache file is NOT overwritten (`fetchedAt` stays `fetchedAtStale`), and the
   rollup still prices to `pricedUsd`. The same rule covers a 2xx body that is
   not valid JSON: never let it poison the cache; fall back to stale.
7. `cached-empty` — `cache-fresh-empty.json` present (fresh age, zero prices).
   A cache payload that parses to zero prices is unusable, same as no cache:
   a fetch is issued (`fetched: true`), returns the canonical payload, the
   cache is rewritten with `fetchedAt = now`, and the rollup prices to
   `pricedUsd`. Matches TS `src/pricing/sources.ts` (cachedCatalog null when
   `prices.size === 0`).
8. `live-reload` — one long-lived pricing instance, empty cacheDir. Query 1 at
   `now` fetches (`fetchedAtFirst = now`) and prices. The clock then advances
   to `nowSecond` (`now + 120000` ms > `ttlMs`); query 2 on the SAME instance
   must consult sources again — the disk cache is now stale, so a second fetch
   is issued and the file's `fetchedAt` advances to `fetchedAtSecond =
   nowSecond`. Total fetches across both queries = `fetchesTotal` (2). Both
   queries price to `pricedUsd`. This pins the TS semantics that catalog loads
   are memoized only within the TTL window (`src/pricing/index.ts`): a
   long-lived instance picks up refreshed market prices; expiry is measured
   from load completion; `refresh` stays one-shot (first load only).

Unusable-payload validation must go through the core parser (via the native
catalog build), never a facade-side reimplementation of source parsers.

`pricedUsd` for the shared rollup (`openai/gpt-5`, input 1000000, output 100000)
is `(1000000 * 1.25 + 100000 * 10) / 1000000 = 2.25`.

## Shared lifecycle constants (`constants.json`)

The three source URLs, the default TTL, the fetch timeout, and the
`pricing-<source>.json` cache-name pattern are data, not lifecycle logic. Each
facade keeps its own native literals (the hybrid boundary guards against
duplicated logic, not duplicated data), so those literals are pinned by a shared
golden contract rather than trusted to stay in agreement by hand. `constants.json`
holds the single source of truth; every facade has a constants-parity test that
loads it and asserts its own constants equal the golden values. A facade whose
literals drift fails against the shared gold rather than silently disagreeing.

The contract is not loaded at runtime and no code is generated from it: it is a
test fixture, consumed only by the parity tests (the same fixture-contract
philosophy as the behavior matrix above).

### Fields

- `sources` -- the three built-in market sources in canonical priority order,
  each with:
  - `name` -- the wire source name, also the cache file stem.
  - `format` -- the core parser format. Note `models-dev`'s format is
    `modelsdev` (no hyphen), distinct from its `name`.
  - `url` -- the endpoint the raw payload is fetched from.
  - `cacheFileName` -- the concrete on-disk cache filename, `pricing-<name>.json`.
- `priorityOrder` -- the source names in the default consultation order
  (OpenRouter > LiteLLM > models.dev), the order `builtin()`/default sources
  return.
- `defaultTtlMs` -- the default cache freshness window in milliseconds (`3600000`,
  i.e. 1 hour). Facades express this as `60 * 60 * 1000`.
- `fetchTimeoutMs` -- the bounded per-fetch timeout in milliseconds (`10000`,
  i.e. 10 s). Facades express this natively (ms, seconds, or a duration) but the
  canonical value is 10000 ms.
- `cacheFileNamePattern` -- the cache-name pattern (`pricing-{name}.json`); the
  three concrete filenames are also carried per source in `sources[].cacheFileName`.

The parity test in each suite loads `constants.json` and asserts the facade's own
source names, formats, URLs, priority order, default TTL, fetch timeout, and the
three cache filenames all equal the golden values.

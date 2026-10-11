---
title: Model matching
description: How a model name from a log becomes a catalog key - the candidate list, the order it is tried in, zero-cost placeholders, and what a miss tells you.
---

## The problem

Logs name models the way each harness does: `claude-sonnet-4-5-20250929`, `anthropic/claude-sonnet-4.5`, `us.anthropic.claude-sonnet-4-5-20250929-v1:0`.
Catalogs key the same model differently again.
Matching bridges the two without a hand-maintained table for every spelling.

## Candidate keys

For each model name, `candidateKeys` builds an ordered list and tries each against the catalog in turn.
The first key the catalog contains wins.

1. The exact name.
2. Provider-prefix strips: each shorter suffix of a `/`-separated path, and vendor route prefixes removed (`us.`, `eu.`, `apac.`, `global.`).
3. Family aliases for vendor decoration, such as Bedrock's `anthropic.claude-sonnet-4-5-20250929-v1:0` reducing to `claude-sonnet-4.5`.
4. Separator normalization (`4.5` against `4-5`) and trailing date-stamp removal (`-20250929`, `-2025-09-29`).
5. Provider-prefixed forms of a family key, so `claude-*` also tries `anthropic/claude-*`.
6. The alias table, applied once (aliases of aliases are not chased).

Order is confidence order.
An exact match is preferred over a normalized one, and a normalized one over an alias, so a catalog that lists both a dated and an undated key returns the dated one for a dated request.

## Sources in priority order

The default sources are OpenRouter, then LiteLLM, then models.dev.
Each source is searched with the full candidate list before the next source is consulted, so a lower-confidence key in a higher-priority source beats an exact key in a lower one.
Overrides you pass to `createPricing` are consulted before any source.

## Zero-cost placeholders

Some catalogs list a model with every rate at 0 as a placeholder.
When the best match in a catalog is such a placeholder, Skopli skips that catalog entirely instead of settling for a lower-confidence key in it, and moves on to the next source.
If every source ends on a placeholder, the result is a miss with `reason: "zeroCost"` and the placeholder's key, so you can tell "unknown model" from "known model, no price listed".

## What a miss contains

```json
{
  "priced": false,
  "model": "my-finetune-v3",
  "attempted": ["my-finetune-v3", "my-finetune-v-3", "my-finetune"]
}
```

`attempted` is the candidate list that was tried, in order, across every source.
Use it to decide whether an override is needed, and to write the override under a key the matcher will actually try.

## Inspecting a match

`lookupModel` returns the hit or miss for one name without pricing any tokens.

```ts tab=lookup
const pricing = createPricing();
const hit = await pricing.lookupModel("claude-sonnet-4-5-20250929");
// { priced: true, model: "claude-sonnet-4-5-20250929", key: "anthropic/claude-sonnet-4.5",
//   price: {...}, source: "openrouter", fetchedAt: "2026-10-11T09:00:00.000Z" }
```

```python tab=lookup
hit = pricing.lookup_model("claude-sonnet-4-5-20250929")
```

```ruby tab=lookup
hit = pricing.lookup_model("claude-sonnet-4-5-20250929")
```

```go tab=lookup
hit, err := pricing.LookupModel("claude-sonnet-4-5-20250929")
```

```java tab=lookup
var hit = pricing.lookupModel("claude-sonnet-4-5-20250929");
```

```csharp tab=lookup
var hit = pricing.LookupModel("claude-sonnet-4-5-20250929");
```

```swift tab=lookup
let hit = try pricing.lookupModel("claude-sonnet-4-5-20250929")
```

```c tab=lookup
ag_buf hit = ag_pricing_lookup_model(pricing, "claude-sonnet-4-5-20250929");
```

`key` is the catalog key that matched, which can differ from the name you asked for.
`fetchedAt` is when that catalog was downloaded; it is `null` for overrides.

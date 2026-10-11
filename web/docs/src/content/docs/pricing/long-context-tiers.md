---
title: Long-context tiers
description: Models whose rates change above a context threshold - how the tier is selected, the two tier modes, and why aggregating before pricing can pick the wrong tier.
---

## What a tier is

Some models charge more once a request's context passes a threshold (200,000 tokens is a common one).
A `ModelPrice` expresses this as `tiers`, each with a `threshold` and its own rates, and a `tierMode`.

```ts
const price: ModelPrice = {
  input: 1.25,
  output: 10,
  tiers: [{ threshold: 200_000, input: 2.5, output: 15 }],
  tierMode: "whole-request",
};
```

## Context size

The tier is chosen by the request's whole context footprint:

```text
context = input + cacheRead + cacheWrite
```

Output does not count toward the threshold.
`cacheWrite1h` is a billing split inside `cacheWrite` and does not add to the context.

## Two tier modes

### `whole-request`

The highest tier whose threshold the context exceeds applies to every token of the request.
Cross the line by one token and the whole request bills at the higher rate.
This is how most vendors describe their long-context pricing.

### `marginal`

Tokens up to the first threshold bill at the base rate, tokens between thresholds bill at that tier's rate, and so on, like income-tax brackets.
Context tokens are blended into one per-token rate (input, cache read and cache write weighted by their counts) so the brackets apply to the footprint as a whole.
Output always bills at the base output rate.

## Tier rates default upward, not sideways

A tier that omits `cacheRead` or `cacheWrite` falls back to its own `input` rate, and a missing `cacheWrite1h` falls back to twice its own `input`.
A tier never inherits a rate from the level below it.

## Why aggregation can pick the wrong tier

Rollup sums tokens across many requests before pricing sees them.
Twenty requests of 50,000 context tokens each sum to 1,000,000, which `priceRollups` would place in the high tier even though no single request crossed 200,000.
The row is flagged:

```json
{ "priced": true, "usd": 4.21, "tieredAggregate": true, "...": "..." }
```

`tieredAggregate: true` means "a tiered price was applied to summed tokens; the per-request answer may be lower".
When you see it, price the events instead.
[Rollups vs events](/skopli/pricing/rollups-vs-events/) shows how.

---
title: Long-context tiers
description: Where tier data comes from, how Skopli picks the tier, how cache writes split per tier, and marginal versus whole-request billing.
---

When a request's context crosses a vendor threshold, the tokens above it bill at the vendor's higher long-context rate. Skopli selects the tier per request and, for Anthropic, reprices the whole request.

## Where tier data comes from

Only the LiteLLM catalog carries tier data (`*_cost_per_token_above_Nk_tokens` fields at the 128k, 200k, 256k, 272k, and 512k boundaries). OpenRouter and models.dev do not.

Skopli selects the tier by the request's total context (`input + cacheRead + cacheWrite`), which matches the vendor definitions.

## Tier inheritance and fallbacks

Within a tier, missing input and output fields inherit the last explicit value from earlier tiers (then the base price). Missing cache fields inherit the last explicit cache rate, and if no catalog entry ever supplies one they fall back to that tier's own input rate, the same fallback the base price uses.

## Cache-write composition per tier

The `cacheWrite` split at 5m and 1h composes with tiers.
At each selected rate level the 1h portion bills at that level's 1h rate (the tier's above-1hr rate when present, else that level's input × 2.0), and the remaining 5m portion bills at that level's `cacheWrite` rate.

See [Price tokens and cache writes](/skopli/pricing/cost-math#cache-write-splits-at-5m-and-1h) for the base split.

## Marginal or whole-request billing

By default tiers are marginal. Context tokens (input, cacheRead, cacheWrite pro-rata) below each threshold bill at the base rates and only the excess at the tier rates, while output always bills at the base output rate. Tier thresholds are context sizes, and output tokens never count toward them.

Source priority between flat and tiered matches is described under [Source priority](/skopli/pricing/model-matching#source-priority).

Anthropic instead reprices the entire request at the long-context rates, so `claude` models with tier data use whole-request semantics. Skopli decides this from the model key, since the catalog carries no explicit repricing signal.

## Error bounds if the heuristic is wrong

If the choice between marginal and whole-request billing is wrong for a model, the difference per crossed boundary is bounded:

- Each context stream (input, cacheRead, cacheWrite) differs by at most `threshold × |tier rate - previous effective rate|`.
- Output differs by `output tokens × |tier output rate - base output rate|`.

For `claude-sonnet-4-5` at the 200k boundary that upper bound is:

| Stream      | Per-request bound | Unit                 |
| ----------- | ----------------: | -------------------- |
| Input       |             $0.60 | per request          |
| Cache read  |             $0.06 | per request          |
| Cache write |             $0.75 | per request          |
| Output      |             $7.50 | per 1M output tokens |

Models with several tiers accumulate one context term per crossed boundary.

When tier rates increase (the usual case), whole-request applied to a marginal model overbills by up to that amount, and marginal applied to a whole-request model underbills by it. The directions flip for any stream whose tier rate decreases.

Requests at or below every threshold price identically in both modes, as do models without tier data.

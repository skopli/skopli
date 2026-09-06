---
title: Long-context tiers
description: Long-context tier rates let large-context requests bill at the vendor's above-threshold rates.
---

When a request's context crosses a vendor threshold, the tokens above it bill at the vendor's higher long-context rate. Skopli selects the tier per request and, for Anthropic, reprices the whole request.

## Where tier data comes from

Only the LiteLLM catalog carries tier data (`*_cost_per_token_above_Nk_tokens` fields at the 128k, 200k, 256k, 272k, and 512k boundaries). OpenRouter and models.dev do not.

The tier is selected by the request's total context (`input + cacheRead + cacheWrite`), matching vendor definitions.

## Tier inheritance and fallbacks

Within a tier, missing input and output fields inherit the last explicit value from earlier tiers (then the base price). Missing cache fields inherit the last explicit cache rate, and if no catalog entry ever supplies one they fall back to that tier's own input rate, mirroring the base fallback.

## Cache-write composition per tier

The cacheWrite 5m/1h split composes with tiers: at each selected rate level the 1h portion bills at that level's 1h rate (the tier's above-1hr rate when present, else that level's input × 2.0) and the remaining 5m portion at that level's cacheWrite rate.

See [Cost math & cache-write splits](/skopli/pricing/cost-math/#cache-write-5m1h-splits) for the base split.

## Marginal vs whole-request

By default tiers are **marginal**. Context tokens (input, cacheRead, cacheWrite pro-rata) below each threshold bill at the base rates and only the excess at the tier rates, while output always bills at the base output rate. Tier thresholds are context sizes, and output tokens never count toward them.

When both catalogs price a model, a flat match yields to any lower-priority match that carries tier data (in practice flat OpenRouter yielding to tiered LiteLLM). Explicit overrides always win.

Anthropic instead reprices the **entire request** at the long-context rates, so `claude` models with tier data use whole-request semantics. This is a heuristic on the model key, since the catalog carries no explicit repricing signal.

## Error bounds if the heuristic is wrong

If the marginal-vs-whole-request heuristic is wrong for a model, the difference per crossed boundary is bounded:

- Each context stream (input, cacheRead, cacheWrite) differs by at most `threshold × |tier rate − previous effective rate|`.
- Output differs by `output tokens × |tier output rate − base output rate|`.

For `claude-sonnet-4-5` at the 200k boundary that upper bound is:

| Stream      | Per-request bound | Unit                 |
| ----------- | ----------------: | -------------------- |
| Input       |             $0.60 | per request          |
| Cache read  |             $0.06 | per request          |
| Cache write |             $0.75 | per request          |
| Output      |             $7.50 | per 1M output tokens |

Models with several tiers accumulate one context term per crossed boundary.

When tier rates increase (the usual case), whole-request applied to a truly-marginal model overbills by up to that amount, and marginal applied to a truly-whole-request model underbills by it; the directions flip for any stream whose tier rate decreases.

Requests at or below every threshold price identically in both modes, as do models without tier data.

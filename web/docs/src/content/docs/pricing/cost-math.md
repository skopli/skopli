---
title: Cost math & cache-write splits
description: Base cost math, disjoint token buckets, and Claude's 5m/1h cache-write split.
---

Prices are USD per million tokens. Each catalog is cached on disk (default ~1 hour TTL) with atomic writes. When a fetch fails, the cache is used at any age, so pricing keeps working offline. Every result says which source priced it and when that data was fetched. Default fetches time out after 10 seconds, so a stalled endpoint never hangs a run. `pricing.catalogs()` reports the loaded catalogs (source, `fetchedAt`, model count), so you can tell when every market source failed and lookups can only miss.

## Base cost math

Reasoning tokens bill at the output rate. Cache rates fall back to the input rate when a catalog omits them.

Token buckets are disjoint. Readers whose sources report reasoning-inclusive output (Codex, Grok, ZCode) subtract reasoning from output before emitting events, so `output + reasoning` reconstructs the source's raw output count and nothing bills twice.

:::caution[One documented assumption]
The OTel GenAI semantic conventions do not state whether `gen_ai.usage.output_tokens` includes reasoning tokens, so the Copilot CLI (OTel) reader assumes the vendor reports them as disjoint and keeps `gen_ai.usage.reasoning*` in its own bucket without subtraction. If Copilot's telemetry turns out to be reasoning-inclusive, its reasoning tokens would be double-billed.
:::

Every rate below is USD per 1M tokens.

| Token bucket | Rate / 1M tokens                     |
| ------------ | ------------------------------------ |
| Input        | catalog input rate                   |
| Output       | catalog output rate                  |
| Reasoning    | catalog output rate                  |
| Cache read   | catalog cache-read rate, else input  |
| Cache write  | catalog cache-write rate, else input |

Reasoning bills at the output rate; cache rates fall back to the input rate when the catalog omits them. Cost for a bucket is `tokens ÷ 1,000,000 × rate`, summed across buckets.

## Cache-write 5m/1h splits

Claude reports a 5m/1h split for ephemeral cache writes (`tokens.cacheWrite1h`, a portion of the `cacheWrite` total):

- **1h writes** bill at the catalog's above-1hr rate when present (LiteLLM's `cache_creation_input_token_cost_above_1hr`), else at input × 2.0 (Anthropic's 1h multiplier).
- The remaining **5m writes** bill at the base `cacheWrite` rate.

### Worked example

The 1h fallback is the only rate this page derives, so the example uses it. Assume a catalog with **input = $3.00 / 1M tokens** and **cache write = $3.75 / 1M tokens**, and a request writing 40,000 cache tokens of which 10,000 are the 1h portion. With no above-1hr rate in the catalog, the 1h rate is `input × 2.0`:

| Cache-write portion | Tokens | Rate / 1M tokens | Formula                |   Cost |
| ------------------- | -----: | ---------------: | ---------------------- | -----: |
| 1h (fallback)       | 10,000 |            $6.00 | 0.010M × ($3.00 × 2.0) |  $0.06 |
| 5m (base)           | 30,000 |            $3.75 | 0.030M × $3.75         | $0.113 |
| **Total**           | 40,000 |                — | —                      | $0.173 |

The $3.00 and $3.75 figures are illustrative catalog values, not a quoted rate for any model; only the `× 2.0` multiplier is Skopli's own documented rule.

Only LiteLLM carries the above-1hr rate. Source priority is per-model, not per-field, so when a higher-priority source such as OpenRouter wins the match, its catalog lacks that field and the input × 2.0 fallback applies. For Anthropic models that fallback equals the published 1h rate, so no accuracy is lost.

The 5m/1h split also composes with long-context tiers. See [Long-context tiers](/skopli/pricing/long-context-tiers/#cache-write-composition-per-tier).

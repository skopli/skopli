---
title: Price tokens and cache writes
description: Base cost math, disjoint token buckets, and Claude's 5-minute and 1-hour cache-write split.
---

Skopli prices each token bucket at its own rate, in USD per million tokens, and sums the buckets.
Every result names the source that priced it and when that catalog was fetched.

## Base cost math

Reasoning tokens bill at the output rate. Cache rates fall back to the input rate when a catalog omits them.

Token buckets are disjoint. Readers whose sources report reasoning-inclusive output (Codex CLI, Grok Build, ZCode) subtract reasoning from output before emitting events, so `output + reasoning` reconstructs the source's raw output count and nothing bills twice.

:::caution[One documented assumption]
The OTel GenAI semantic conventions do not state whether `gen_ai.usage.output_tokens` includes reasoning tokens, so the GitHub Copilot CLI reader assumes the vendor reports them as disjoint and keeps `gen_ai.usage.reasoning*` in its own bucket without subtraction. If Copilot's telemetry turns out to be reasoning-inclusive, its reasoning tokens would be double-billed.
:::

| Token bucket | Rate per 1M tokens                   |
| ------------ | ------------------------------------ |
| Input        | catalog input rate                   |
| Output       | catalog output rate                  |
| Reasoning    | catalog output rate                  |
| Cache read   | catalog cache-read rate, else input  |
| Cache write  | catalog cache-write rate, else input |

Cost for a bucket is `tokens ÷ 1,000,000 × rate`, summed across buckets.

## Cache-write splits at 5m and 1h

Claude reports 5-minute and 1-hour ephemeral cache writes separately (`tokens.cacheWrite1h` is the 1-hour portion of the `cacheWrite` total):

- 1h writes bill at the catalog's above-1hr rate when present (LiteLLM's `cache_creation_input_token_cost_above_1hr`), else at input × 2.0 (Anthropic's 1h multiplier).
- The remaining 5m writes bill at the base `cacheWrite` rate.

### Worked example

The 1h fallback is the only rate this page derives, so the example uses it. Assume a catalog with input at $3.00 per 1M tokens and cache write at $3.75 per 1M tokens, and a request writing 40,000 cache tokens of which 10,000 are the 1h portion. With no above-1hr rate in the catalog, the 1h rate is `input × 2.0`:

| Cache-write portion | Tokens | Rate per 1M tokens | Formula                |   Cost |
| ------------------- | -----: | -----------------: | ---------------------- | -----: |
| 1h (fallback)       | 10,000 |              $6.00 | 0.010M × ($3.00 × 2.0) | $0.060 |
| 5m (base)           | 30,000 |              $3.75 | 0.030M × $3.75         | $0.113 |
| **Total**           | 40,000 |                    |                        | $0.173 |

The $3.00 and $3.75 figures are illustrative catalog values.
Only the `× 2.0` multiplier is Skopli's own documented rule.

Only LiteLLM carries the above-1hr rate. Source priority is per-model, not per-field, so when a higher-priority source such as OpenRouter wins the match, its catalog lacks that field and the input × 2.0 fallback applies. For Anthropic models that fallback equals the published 1h rate, so no accuracy is lost.

The cache-write split also composes with long-context tiers. See [Long-context tiers](/skopli/pricing/long-context-tiers#cache-write-composition-per-tier).

---
title: Cost math
description: How Skopli turns token buckets and catalog rates into dollars, including reasoning, cache reads, and the 5m/1h cache-write split.
---

All rates are USD per million tokens.
For a flat-rate model the cost of a token bundle is the sum over buckets of `tokens x rate / 1,000,000`.

## Bucket rates

| Bucket         | Rate used                                                                  |
| -------------- | -------------------------------------------------------------------------- |
| `input`        | `price.input`                                                              |
| `output`       | `price.output`                                                             |
| `reasoning`    | `price.output`                                                             |
| `cacheRead`    | `price.cacheRead`, falling back to `price.input` when the catalog omits it |
| `cacheWrite`   | `price.cacheWrite`, falling back to `price.input` when omitted             |
| `cacheWrite1h` | `price.cacheWrite1h`, falling back to `price.input x 2` when omitted       |

Buckets are disjoint, so nothing is billed twice.
`cacheWrite` is the total across cache TTLs; `cacheWrite1h` is the part of it written with a one-hour TTL.
Only the remainder (`cacheWrite - cacheWrite1h`) bills at the 5-minute `cacheWrite` rate.
Source priority is per model, not per field: once a catalog wins the match, all of its rates are used, including its omissions.

## A flat example

From the repository's golden fixture, `gpt-5` with 1,000 input and 500 output tokens at LiteLLM's rates of 1.25 and 10.00:

```text
1,000 x 1.25 / 1e6 = 0.00125
  500 x 10.00 / 1e6 = 0.00500
                       0.00625
```

## A Claude example with the 1h split and a tier

The same fixture prices `claude-sonnet-4-5` with these tokens:

| Bucket         |  Tokens |
| -------------- | ------: |
| `input`        | 205,000 |
| `output`       |  11,000 |
| `cacheRead`    |  22,000 |
| `cacheWrite`   |  34,000 |
| `cacheWrite1h` |  11,500 |
| `reasoning`    |   2,000 |

Context for tier selection is `input + cacheRead + cacheWrite` = 261,000, which crosses the 200k tier.
Claude models reprice the whole request at the tier's rates (input 6.00, output 22.50, cacheRead 0.60, cacheWrite 7.50, cacheWrite1h 12.00):

```text
205,000 x  6.00 = 1,230,000
 11,000 x 22.50 =   247,500
  2,000 x 22.50 =    45,000   reasoning at the output rate
 22,000 x  0.60 =    13,200
 22,500 x  7.50 =   168,750   cacheWrite minus the 1h portion
 11,500 x 12.00 =   138,000   the 1h portion
                  1,842,450 / 1e6 = 1.84245
```

Those are synthetic token counts against a catalog snapshot dated 2026-08-01; the arithmetic is what matters.
[Long-context tiers](/skopli/pricing/long-context-tiers/) covers how the tier was chosen.

## costUsd

`costUsd` on events and rollups is the amount the harness itself recorded, where it recorded one.
It is kept separate from the computed price so you can compare the two, and `mode: "display"` or `"auto"` lets you prefer it.

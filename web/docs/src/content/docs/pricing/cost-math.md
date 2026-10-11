---
title: Cost math
description: The exact formula behind costUsd - rates per million tokens, reasoning billed as output, cache read and write rates with their defaults, and the 1-hour cache-write split.
---

## The formula

For a flat price (no long-context tiers):

```text
usd = ( input      x price.input
      + (output + reasoning) x price.output
      + cacheRead  x price.cacheRead
      + cacheWrite5m x price.cacheWrite
      + cacheWrite1h x price.cacheWrite1h ) / 1,000,000
```

Rates are USD per million tokens, which is how every catalog publishes them.

## Reasoning bills as output

Vendors charge reasoning tokens at the output rate, so `costUsd` adds `reasoning` to `output` before multiplying.
The two counters stay separate in the event so you can still see how much of the output bill was thinking.

## Cache rates and defaults

Catalogs do not always list cache rates.
When one is missing:

- `cacheRead` defaults to the `input` rate.
- `cacheWrite` defaults to the `input` rate.
- `cacheWrite1h` defaults to twice this level's `input` rate.

Defaults are conservative in the direction of overcharging cache reads, which vendors usually discount.
If a model's cache discount matters to you and the catalog omits it, pass an override with the real rates.

## The 1-hour cache-write split

Some vendors offer two cache TTLs with different write prices.
A reader that can see which writes used the long TTL fills `cacheWrite1h`; it is a portion of `cacheWrite`, not an extra bucket.

```text
cacheWrite1h = clamp(tokens.cacheWrite1h, 0, tokens.cacheWrite)
cacheWrite5m = tokens.cacheWrite - cacheWrite1h
```

A missing, negative or non-finite `cacheWrite1h` counts as 0, so a malformed log cannot bill more writes than were made.

## Worked example

Tokens: 120,000 input, 8,000 output, 2,000 reasoning, 400,000 cache read, 50,000 cache write of which 10,000 at 1h.
Price: input 3, output 15, cacheRead 0.3, cacheWrite 3.75, cacheWrite1h 6.

```text
input       120,000 x 3     =   360,000
output       10,000 x 15    =   150,000
cacheRead   400,000 x 0.3   =   120,000
cacheWrite5m 40,000 x 3.75  =   150,000
cacheWrite1h 10,000 x 6     =    60,000
sum                         =   840,000  / 1e6  = 0.84 USD
```

## Rounding

`costUsd` returns a floating-point number and does no rounding.
Round at the edge of your own output, and sum in full precision before rounding.

## Tiers

When a price carries `tiers`, the rate depends on the request's context size.
[Long-context tiers](/skopli/pricing/long-context-tiers/) covers both tier modes.

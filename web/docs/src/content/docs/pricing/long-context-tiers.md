---
title: Long-context tiers
description: How tiered catalog entries change the rate once a request's context crosses a threshold, and the two ways Skopli applies them.
---

Some models charge more once a request's context exceeds a threshold.
LiteLLM is the only shipped catalog that publishes these tiers.
OpenRouter and models.dev entries are flat.
Skopli reads thresholds at 128k, 200k, 256k, 272k and 512k tokens.

## Tier selection

The context used to pick a tier is `input + cacheRead + cacheWrite`.
Output and reasoning tokens do not count toward it.

```ts
type PriceTier = {
  threshold: number; // context above which this tier applies
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
  cacheWrite1h?: number; // falls back to this tier's input x 2, never to a lower tier's value
};

type ModelPrice = {
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
  cacheWrite1h?: number;
  tiers?: PriceTier[];
  tierMode?: "marginal" | "whole-request";
};
```

## Marginal vs whole-request

**Marginal** billing is the default.
Tokens up to the first threshold bill at the base rate, tokens between thresholds bill at that tier's rate, and so on, the way income tax brackets work.

**Whole-request** billing applies to Claude models.
Once context crosses a threshold, every token in the request bills at that tier's rates.
This is how Anthropic prices long-context requests, and it is why the [cost math example](/skopli/pricing/cost-math/) multiplies all 205,000 input tokens by the tier rate rather than only the 61,000 above the line.

The cache-write split composes per tier: the 1h portion bills at the selected tier's `cacheWrite1h`, the 5m remainder at its `cacheWrite`.

## Tiered entries win

After overrides, a match that carries tiers outranks a flat match from a higher-priority source.
In practice LiteLLM's tiered Claude entries beat OpenRouter's flat ones, so you get the tiers even though OpenRouter is consulted first.

## Tiers need per-event pricing

A rollup sums tokens across many events.
Pricing that sum can cross a tier none of the individual requests reached and overbill.
`priceRollups` flags such rows with `tieredAggregate: true`.
`priceEvents` avoids the problem by pricing each event at its own context.
See [Rollups vs events](/skopli/pricing/rollups-vs-events/).

export interface TokenCounts {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cacheWrite1h?: number;
  reasoning: number;
}

export interface PriceTier {
  threshold: number;
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
  cacheWrite1h?: number;
}

export interface ModelPrice {
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
  cacheWrite1h?: number;
  tiers?: PriceTier[];
  tierMode?: "whole-request" | "marginal";
}

interface Rates {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cacheWrite1h: number;
}

function rateSchedule(price: ModelPrice): {
  base: Rates;
  steps: { threshold: number; rates: Rates }[];
} {
  const tiers = [...(price.tiers ?? [])].sort((a, b) => a.threshold - b.threshold);
  let read = price.cacheRead;
  let write = price.cacheWrite;
  const base = {
    input: price.input,
    output: price.output,
    cacheRead: read ?? price.input,
    cacheWrite: write ?? price.input,
    cacheWrite1h: price.cacheWrite1h ?? price.input * 2,
  };
  const steps = tiers.map((tier) => {
    read = tier.cacheRead ?? read;
    write = tier.cacheWrite ?? write;
    return {
      threshold: tier.threshold,
      rates: {
        input: tier.input,
        output: tier.output,
        cacheRead: read ?? tier.input,
        cacheWrite: write ?? tier.input,
        cacheWrite1h: tier.cacheWrite1h ?? tier.input * 2,
      },
    };
  });
  return { base, steps };
}

function marginalCost(tokens: number, base: number, points: { threshold: number; rate: number }[]) {
  let usd = 0;
  let prev = 0;
  let rate = base;
  for (const point of points) {
    if (tokens <= point.threshold) break;
    usd += (point.threshold - prev) * rate;
    prev = point.threshold;
    rate = point.rate;
  }
  return usd + (tokens - prev) * rate;
}

/** Same arithmetic, in the same order, as `costUsd` in the SDK's `src/pricing/index.ts`. */
export function costUsd(tokens: TokenCounts, price: ModelPrice): number {
  const output = tokens.output + tokens.reasoning;
  const { base, steps } = rateSchedule(price);
  const context = tokens.input + tokens.cacheRead + tokens.cacheWrite;
  const reported1h = tokens.cacheWrite1h;
  const finite1h = reported1h !== undefined && Number.isFinite(reported1h) ? reported1h : 0;
  const write1h = Math.min(Math.max(finite1h, 0), Math.max(tokens.cacheWrite, 0));
  const write5m = tokens.cacheWrite - write1h;
  const cacheWriteCost = (r: Rates) => write5m * r.cacheWrite + write1h * r.cacheWrite1h;
  const bill = (r: Rates) =>
    (tokens.input * r.input +
      output * r.output +
      tokens.cacheRead * r.cacheRead +
      cacheWriteCost(r)) /
    1_000_000;
  if (steps.length === 0) return bill(base);
  if (price.tierMode === "whole-request") {
    let active = base;
    for (const step of steps) {
      if (context <= step.threshold) break;
      active = step.rates;
    }
    return bill(active);
  }
  const blend = (r: Rates) =>
    context === 0
      ? 0
      : (tokens.input * r.input + tokens.cacheRead * r.cacheRead + cacheWriteCost(r)) / context;
  return (
    (marginalCost(
      context,
      blend(base),
      steps.map((s) => ({ threshold: s.threshold, rate: blend(s.rates) })),
    ) +
      output * base.output) /
    1_000_000
  );
}

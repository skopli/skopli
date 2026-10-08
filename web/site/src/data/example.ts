import { costUsd } from "../../../../src/pricing/index.ts";
import type { ModelPrice } from "../../../../src/pricing/types.ts";
import type { TokenCounts } from "../../../../src/types.ts";

export const rateDate = "2026-08-01";

export type Catalog = "openrouter" | "litellm";

export interface PricedRow {
  model: string;
  key: string;
  tokens: TokenCounts;
  pricing: { priced: true; source: Catalog; price: ModelPrice; usd: number };
}

export interface MissRow {
  model: string;
  tokens: TokenCounts;
  pricing: { priced: false; attempted: string[] };
}

export type Row = PricedRow | MissRow;

export const isPriced = (row: Row): row is PricedRow => row.pricing.priced;

const priced = (
  model: string,
  key: string,
  tokens: TokenCounts,
  source: Catalog,
  price: ModelPrice,
): PricedRow => ({
  model,
  key,
  tokens,
  pricing: { priced: true, source, price, usd: costUsd(tokens, price) },
});

export const rows: Row[] = [
  priced(
    "claude-sonnet-4-5",
    "claude-sonnet-4-5",
    {
      input: 8_412_006,
      output: 1_864_442,
      cacheRead: 31_004_998,
      cacheWrite: 1_786_114,
      cacheWrite1h: 412_300,
      reasoning: 0,
    },
    "litellm",
    {
      input: 3,
      output: 15,
      cacheRead: 0.3,
      cacheWrite: 3.75,
      cacheWrite1h: 6,
      tierMode: "whole-request",
      tiers: [
        {
          threshold: 200_000,
          input: 6,
          output: 22.5,
          cacheRead: 0.6,
          cacheWrite: 7.5,
          cacheWrite1h: 12,
        },
      ],
    },
  ),
  priced(
    "openai/gpt-5",
    "openai/gpt-5",
    { input: 5_120_338, output: 922_378, cacheRead: 13_204_500, cacheWrite: 0, reasoning: 0 },
    "openrouter",
    { input: 1.25, output: 10, cacheRead: 0.125 },
  ),
  priced(
    "claude-opus-4.6",
    "anthropic/claude-opus-4.6",
    { input: 2_887_412, output: 603_266, cacheRead: 6_902_776, cacheWrite: 761_716, reasoning: 0 },
    "openrouter",
    { input: 5, output: 25 },
  ),
  {
    model: "local-fine-tune",
    tokens: {
      input: 4_002_118,
      output: 401_092,
      cacheRead: 2_100_140,
      cacheWrite: 230_152,
      reasoning: 0,
    },
    pricing: { priced: false, attempted: ["local-fine-tune"] },
  },
];

const pricedRows = rows.filter(isPriced);
const sum = (pick: (row: PricedRow) => number) =>
  pricedRows.reduce((acc, row) => acc + pick(row), 0);

export const totals = {
  input: sum((r) => r.tokens.input),
  cacheRead: sum((r) => r.tokens.cacheRead),
  cacheWrite: sum((r) => r.tokens.cacheWrite),
  output: sum((r) => r.tokens.output),
  usd: sum((r) => r.pricing.usd),
  priced: pricedRows.length,
  misses: rows.length - pricedRows.length,
};

export const usd = (value: number) =>
  value.toLocaleString("en-US", { style: "currency", currency: "USD" });
export const int = (value: number) => value.toLocaleString("en-US");

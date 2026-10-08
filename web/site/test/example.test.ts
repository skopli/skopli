import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { costUsd } from "../../../src/pricing/index.ts";
import type { ModelPrice } from "../../../src/pricing/types.ts";
import { isPriced, rateDate, rows, totals } from "../src/data/example.ts";

const repo = new URL("../../../", import.meta.url);
const json = (path: string) => JSON.parse(readFileSync(new URL(path, repo), "utf8"));
const perMillion = (perToken: number) => perToken * 1_000_000;

describe("landing ledger", () => {
  it("prices every row to the hand-computed USD figure", () => {
    const expected: Record<string, number> = {
      "claude-sonnet-4-5": 126.2761848,
      "gpt-5": 17.274765,
      "claude-opus-4.6": 67.84117,
    };
    for (const row of rows.filter(isPriced)) {
      expect(row.pricing.usd).toBeCloseTo(expected[row.model], 7);
    }
    expect(totals.usd).toBeCloseTo(211.3921198, 7);
  });

  it("counts the miss and keeps it out of every total", () => {
    expect(rows.filter((r) => !isPriced(r))).toHaveLength(1);
    expect(totals.misses).toBe(1);
    expect(totals.priced).toBe(3);
    expect(totals.input).toBe(16_419_756);
    expect(totals.cacheRead).toBe(51_112_274);
    expect(totals.cacheWrite).toBe(2_547_830);
    expect(totals.output).toBe(3_390_086);
  });

  it("records the rates from the committed catalog snapshots", () => {
    const litellm = json("golden/pricing/catalogs/litellm.json");
    const openrouter = json("golden/pricing/catalogs/openrouter.json").data;
    for (const row of rows.filter(isPriced)) {
      const { price, source } = row.pricing;
      if (source === "litellm") {
        const entry = litellm[row.key];
        expect(price.input).toBeCloseTo(perMillion(entry.input_cost_per_token), 12);
        expect(price.output).toBeCloseTo(perMillion(entry.output_cost_per_token), 12);
        expect(price.cacheRead).toBeCloseTo(perMillion(entry.cache_read_input_token_cost), 12);
        expect(price.cacheWrite).toBeCloseTo(perMillion(entry.cache_creation_input_token_cost), 12);
        expect(price.cacheWrite1h).toBeCloseTo(
          perMillion(entry.cache_creation_input_token_cost_above_1hr),
          12,
        );
        const tier = price.tiers?.[0];
        expect(tier?.threshold).toBe(200_000);
        expect(tier?.input).toBeCloseTo(
          perMillion(entry.input_cost_per_token_above_200k_tokens),
          12,
        );
        expect(tier?.output).toBeCloseTo(
          perMillion(entry.output_cost_per_token_above_200k_tokens),
          12,
        );
      } else {
        const entry = openrouter.find((m: { id: string }) => m.id === row.key).pricing;
        expect(price.input).toBeCloseTo(perMillion(Number(entry.prompt)), 12);
        expect(price.output).toBeCloseTo(perMillion(Number(entry.completion)), 12);
        if (entry.input_cache_read !== undefined) {
          expect(price.cacheRead).toBeCloseTo(perMillion(Number(entry.input_cache_read)), 12);
        } else {
          expect(price.cacheRead).toBeUndefined();
        }
      }
    }
  });

  it("matches the pricing conformance gold, which pins the rate date", () => {
    const gold = json("golden/pricing/basic/expected-priced-rollup.json");
    for (const rollup of gold.rollups) {
      if (!rollup.pricing.priced) continue;
      expect(rollup.pricing.fetchedAt.startsWith(rateDate)).toBe(true);
      expect(costUsd(rollup.tokens, rollup.pricing.price as ModelPrice)).toBe(rollup.pricing.usd);
    }
  });
});

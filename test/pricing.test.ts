import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readUsage } from "../src/read.ts";
import { loadCached, storeCached } from "../src/pricing/cache.ts";
import { candidateKeys, matchModel } from "../src/pricing/match.ts";
import { DEFAULT_TTL_MS, FETCH_TIMEOUT_MS, costUsd, createPricing } from "../src/pricing/index.ts";
import type { PricingMode } from "../src/pricing/index.ts";
import {
  BUILTIN_SOURCE_FORMATS,
  FORMAT_PARSERS,
  LITELLM_URL,
  MODELS_DEV_URL,
  OPENROUTER_URL,
  cacheFileName,
  liteLlmSource,
  modelsDevSource,
  openRouterSource,
} from "../src/pricing/sources.ts";
import type { BuiltinFormat } from "../src/pricing/sources.ts";
import type { FetchLike, PricingSource } from "../src/pricing/types.ts";
import { rollup } from "../src/rollup.ts";
import type { Rollup } from "../src/rollup.ts";
import type { UsageEvent } from "../src/types.ts";

function tempDir(): string {
  return mkdtempSync(join(tmpdir(), "skopli-pricing-"));
}

function fetchStub(routes: Record<string, unknown>): { fetch: FetchLike; calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    fetch: (url: string) => {
      calls.push(url);
      const payload = routes[url];
      if (payload === undefined) {
        return Promise.resolve({
          ok: false,
          status: 404,
          json: () => Promise.reject(new Error("no body")),
        });
      }
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(payload) });
    },
  };
}

const failingFetch: FetchLike = () => Promise.reject(new Error("network down"));

const OPENROUTER_PAYLOAD = {
  data: [
    {
      id: "openai/gpt-5",
      pricing: {
        prompt: "0.00000125",
        completion: "0.00001",
        input_cache_read: "0.000000125",
      },
    },
    { id: "anthropic/claude-sonnet-4.5", pricing: { prompt: "0.000003", completion: "0.000015" } },
    { id: "anthropic/claude-opus-4.6", pricing: { prompt: "0.000005", completion: "0.000025" } },
    { id: "broken", pricing: { prompt: "not-a-number", completion: "1" } },
  ],
};

const LITELLM_PAYLOAD = {
  sample_spec: { input_cost_per_token: 0 },
  "gpt-5": { input_cost_per_token: 0.00000125, output_cost_per_token: 0.00001 },
  "claude-sonnet-4-5": {
    input_cost_per_token: 0.000003,
    output_cost_per_token: 0.000015,
    cache_read_input_token_cost: 3e-7,
    cache_creation_input_token_cost: 0.00000375,
    cache_creation_input_token_cost_above_1hr: 0.000006,
    input_cost_per_token_above_200k_tokens: 0.000006,
    output_cost_per_token_above_200k_tokens: 0.0000225,
    cache_read_input_token_cost_above_200k_tokens: 6e-7,
    cache_creation_input_token_cost_above_200k_tokens: 0.0000075,
    cache_creation_input_token_cost_above_1hr_above_200k_tokens: 0.000012,
  },
  "gemini-tiered": {
    input_cost_per_token: 0.00000125,
    output_cost_per_token: 0.00001,
    input_cost_per_token_above_128k_tokens: 0.0000025,
    output_cost_per_token_above_128k_tokens: 0.00002,
  },
  "cache-only-tiered": {
    input_cost_per_token: 0.000001,
    output_cost_per_token: 0.000005,
    cache_read_input_token_cost_above_256k_tokens: 5e-7,
  },
  "multi-tiered": {
    input_cost_per_token: 0.000001,
    output_cost_per_token: 0.000005,
    input_cost_per_token_above_128k_tokens: 0.000002,
    cache_read_input_token_cost_above_128k_tokens: 2.5e-7,
    output_cost_per_token_above_256k_tokens: 0.00001,
  },
  "wide-tiered": {
    input_cost_per_token: 0.000002,
    output_cost_per_token: 0.00001,
    input_cost_per_token_above_272k_tokens: 0.000004,
    cache_creation_input_token_cost_above_272k_tokens: 0.000005,
    output_cost_per_token_above_512k_tokens: 0.00002,
    cache_read_input_token_cost_above_512k_tokens: 5e-7,
  },
  "bad-1hr-rate": {
    input_cost_per_token: 0.000001,
    output_cost_per_token: 0.000002,
    cache_creation_input_token_cost_above_1hr: -0.000001,
  },
  "claude-opus-4.6": { input_cost_per_token: 0.000005, output_cost_per_token: 0.000025 },
};

const MODELS_DEV_PAYLOAD = {
  openai: { models: { "gpt-5": { cost: { input: 1.25, output: 10 } } } },
  moonshotai: {
    models: {
      "kimi-k3": { cost: { input: 0.6, output: 2.5, cache_read: 0.15 } },
    },
  },
  "amazon-bedrock": {
    models: {
      "anthropic.claude-opus-4-6-20260115-v1:0": { cost: { input: 0, output: 0 } },
      "anthropic.mystery-9-v1:0": { cost: { input: 0, output: 0 } },
    },
  },
};

const zeroTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0 };

describe("candidateKeys", () => {
  it("tries exact, prefix-stripped, and normalized variants in order", () => {
    const keys = candidateKeys("us.anthropic.claude-sonnet-4-5-20250929-v1:0");
    expect(keys[0]).toBe("us.anthropic.claude-sonnet-4-5-20250929-v1:0");
    expect(keys).toContain("anthropic.claude-sonnet-4-5-20250929-v1:0");
    expect(keys).toContain("claude-sonnet-4.5");
  });

  it("normalizes separators and date stamps", () => {
    expect(candidateKeys("claude-sonnet-4.5")).toContain("claude-sonnet-4-5");
    expect(candidateKeys("claude-sonnet-4-5-20250929")).toContain("claude-sonnet-4-5");
    expect(candidateKeys("anthropic/claude-sonnet-4-5")).toContain("claude-sonnet-4.5");
  });
});

describe("matchModel", () => {
  it("returns the attempted keys on a miss", () => {
    const result = matchModel("mystery-model", new Map());
    expect("attempted" in result && result.attempted).toContain("mystery-model");
  });

  it("aliases Bedrock claude-family ids to claude keys", () => {
    const keys = candidateKeys("us.anthropic.claude-opus-4-6-20260115-v1:0");
    expect(keys).toContain("claude-opus-4.6");
    expect(keys).toContain("claude-opus-4-6");
    expect(keys).toContain("anthropic/claude-opus-4.6");
    expect(candidateKeys("anthropic.claude-sonnet-5-1-v1")).toContain("claude-sonnet-5.1");
  });

  it("skips all-zero entries when asked, reporting the zero-cost key", () => {
    const prices = new Map([["freebie", { input: 0, output: 0 }]]);
    const skipped = matchModel("freebie", prices, { skipZeroCost: true });
    expect(skipped).toMatchObject({ zeroCostKey: "freebie" });
    const kept = matchModel("freebie", prices);
    expect(kept).toMatchObject({ key: "freebie" });
  });

  it("abandons the whole catalog when the best match is zero-cost", () => {
    const prices = new Map([
      ["anthropic.claude-opus-4-6-v1:0", { input: 0, output: 0 }],
      ["claude-opus-4.6", { input: 5, output: 25 }],
    ]);
    const result = matchModel("anthropic.claude-opus-4-6-v1:0", prices, { skipZeroCost: true });
    expect(result).toMatchObject({ zeroCostKey: "anthropic.claude-opus-4-6-v1:0" });
    expect("price" in result).toBe(false);
  });

  it("keeps an entry whose only nonzero rate is the 1h cache-write rate", () => {
    const prices = new Map([["oddball", { input: 0, output: 0, cacheWrite1h: 6 }]]);
    const result = matchModel("oddball", prices, { skipZeroCost: true });
    expect(result).toMatchObject({ key: "oddball" });
  });

  it("keeps an entry whose base rates are zero but a long-context tier is paid", () => {
    const prices = new Map([
      [
        "tiered-free-base",
        { input: 0, output: 0, tiers: [{ threshold: 200_000, input: 6, output: 22.5 }] },
      ],
    ]);
    const result = matchModel("tiered-free-base", prices, { skipZeroCost: true });
    expect(result).toMatchObject({ key: "tiered-free-base" });
  });
});

describe("costUsd", () => {
  it("bills a codex event at input*rate + raw output_tokens*outRate with no double count", async () => {
    const root = mkdtempSync(join(tmpdir(), "skopli-codex-pricing-"));
    try {
      mkdirSync(join(root, "sessions"));
      writeFileSync(
        join(root, "sessions", "rollout-priced.jsonl"),
        `${[
          { type: "session_meta", payload: { id: "priced" } },
          { type: "turn_context", payload: { model: "gpt-5.1-codex" } },
          {
            type: "event_msg",
            timestamp: "2026-08-01T00:00:10.000Z",
            payload: {
              type: "token_count",
              info: {
                last_token_usage: {
                  input_tokens: 1_000_000,
                  output_tokens: 500_000,
                  reasoning_output_tokens: 200_000,
                },
              },
            },
          },
        ]
          .map((line) => JSON.stringify(line))
          .join("\n")}\n`,
      );
      const { events } = await readUsage({
        home: root,
        env: { CODEX_HOME: root },
        harnesses: ["codex"],
      });
      expect(events).toHaveLength(1);
      const tokens = events[0]?.tokens;
      expect(tokens).toBeDefined();
      if (tokens === undefined) return;
      expect(tokens.output + tokens.reasoning).toBe(500_000);
      const usd = costUsd(tokens, { input: 2, output: 10 });
      expect(usd).toBeCloseTo((1_000_000 * 2 + 500_000 * 10) / 1_000_000, 10);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("bills reasoning as output and falls back cache tiers to input rate", () => {
    const usd = costUsd(
      {
        input: 1_000_000,
        output: 500_000,
        cacheRead: 200_000,
        cacheWrite: 100_000,
        reasoning: 500_000,
      },
      { input: 2, output: 10 },
    );
    expect(usd).toBeCloseTo(2 + 10 + 0.4 + 0.2, 10);
  });

  it("bills 5m and 1h cache writes to the exact Anthropic formula", () => {
    const tokens = {
      input: 1_000_000,
      output: 500_000,
      cacheRead: 400_000,
      cacheWrite: 900_000,
      cacheWrite1h: 300_000,
      reasoning: 0,
    };
    const usd = costUsd(tokens, { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 });
    expect(usd).toBeCloseTo(3 + 7.5 + 0.12 + 0.6 * 3.75 + 0.3 * 6, 10);
  });

  it("prefers the catalog above-1hr rate over the input x 2 fallback", () => {
    const tokens = {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 1_000_000,
      cacheWrite1h: 1_000_000,
      reasoning: 0,
    };
    const usd = costUsd(tokens, { input: 3, output: 15, cacheWrite: 3.75, cacheWrite1h: 5 });
    expect(usd).toBeCloseTo(5, 10);
  });

  it("prices aggregate-only cache writes identically to before the split", () => {
    const tokens = {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 900_000,
      reasoning: 0,
    };
    const usd = costUsd(tokens, { input: 3, output: 15, cacheWrite: 3.75, cacheWrite1h: 6 });
    expect(usd).toBeCloseTo(0.9 * 3.75, 10);
  });

  it("clamps a malformed 1h split into the cacheWrite total", () => {
    const tokens = {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 100_000,
      cacheWrite1h: 900_000,
      reasoning: 0,
    };
    const usd = costUsd(tokens, { input: 3, output: 15, cacheWrite: 3.75 });
    expect(usd).toBeCloseTo(0.1 * 6, 10);
  });

  it("treats non-finite 1h splits as zero instead of poisoning the cost", () => {
    const tokens = {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 100_000,
      reasoning: 0,
    };
    const price = { input: 3, output: 15, cacheWrite: 3.75 };
    const base = costUsd(tokens, price);
    expect(costUsd({ ...tokens, cacheWrite1h: Number.NaN }, price)).toBeCloseTo(base, 10);
    expect(costUsd({ ...tokens, cacheWrite1h: Number.POSITIVE_INFINITY }, price)).toBeCloseTo(
      base,
      10,
    );
  });

  it("keeps aggregate-only pricing on a negative cacheWrite total", () => {
    const tokens = {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: -100_000,
      reasoning: 0,
    };
    const usd = costUsd(tokens, { input: 3, output: 15, cacheWrite: 3.75 });
    expect(usd).toBeCloseTo(-0.1 * 3.75, 10);
  });

  it("prices the claude 5m/1h fixture to the exact Anthropic formula", async () => {
    const fixtures = join(dirname(fileURLToPath(import.meta.url)), "fixtures");
    const { events } = await readUsage({
      home: fixtures,
      env: { CLAUDE_CONFIG_DIR: join(fixtures, "claude") },
      harnesses: ["claude"],
    });
    const event = events.find((e) => e.messageId === "msg_d1");
    expect(event).toBeDefined();
    if (event === undefined) return;
    const usd = costUsd(event.tokens, { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 });
    const expected = (1000 * 3 + 500 * 15 + 400 * 0.3 + 600 * 3.75 + 300 * (3 * 2)) / 1_000_000;
    expect(usd).toBeCloseTo(expected, 12);
  });

  const sonnetTokens = {
    input: 250_000,
    output: 5_000,
    cacheRead: 0,
    cacheWrite: 0,
    reasoning: 0,
  };
  const sonnetTier = { threshold: 200_000, input: 6, output: 22.5 };

  it("prices the worked example flat without tiers", () => {
    expect(costUsd(sonnetTokens, { input: 3, output: 15 })).toBeCloseTo(0.825, 10);
  });

  it("prices the worked example marginally by default", () => {
    const usd = costUsd(sonnetTokens, { input: 3, output: 15, tiers: [sonnetTier] });
    expect(usd).toBeCloseTo(0.975, 10);
  });

  it("prices the worked example whole-request when the model reprices", () => {
    const usd = costUsd(sonnetTokens, {
      input: 3,
      output: 15,
      tiers: [sonnetTier],
      tierMode: "whole-request",
    });
  });

  describe("probe conformance (shared fixtures)", () => {
    const casesDir = join(
      dirname(fileURLToPath(import.meta.url)),
      "..",
      "golden",
      "pricing",
      "probe",
    );

    type Case = {
      name: string;
      format: BuiltinFormat;
      payload?: unknown;
      payloadRaw?: string;
      usable: boolean;
    };

    const cases = JSON.parse(readFileSync(join(casesDir, "cases.json"), "utf8")) as Case[];

    // The TS equivalent of the native probe is "parse to zero prices is unusable":
    // a payload is usable when its format parser yields at least one price. An
    // invalid-JSON payloadRaw fails to parse to a value, so it parses to zero.
    function usable(kase: Case): boolean {
      let value: unknown;
      if (kase.payloadRaw !== undefined) {
        try {
          value = JSON.parse(kase.payloadRaw);
        } catch {
          return false;
        }
      } else {
        value = kase.payload;
      }
      return FORMAT_PARSERS[kase.format](value).size > 0;
    }

    for (const kase of cases) {
      it(kase.name, () => {
        expect(usable(kase)).toBe(kase.usable);
      });
    }
  });

  it("selects the tier from total context including cache tokens", () => {
    const usd = costUsd(
      { input: 100_000, output: 1_000, cacheRead: 80_000, cacheWrite: 40_000, reasoning: 0 },
      {
        input: 3,
        output: 15,
        cacheRead: 0.3,
        cacheWrite: 3.75,
        tiers: [{ threshold: 200_000, input: 6, output: 22.5, cacheRead: 0.6, cacheWrite: 7.5 }],
        tierMode: "whole-request",
      },
    );
    expect(usd).toBeCloseTo((100_000 * 6 + 1_000 * 22.5 + 80_000 * 0.6 + 40_000 * 7.5) / 1e6, 10);
  });

  it("crosses marginal thresholds on total context including cache tokens", () => {
    const usd = costUsd(
      { input: 100_000, output: 0, cacheRead: 110_000, cacheWrite: 40_000, reasoning: 0 },
      {
        input: 3,
        output: 15,
        cacheRead: 0.3,
        cacheWrite: 3.75,
        tiers: [{ threshold: 200_000, input: 6, output: 22.5, cacheRead: 0.6, cacheWrite: 7.5 }],
      },
    );
    const context = 250_000;
    const baseBlend = (100_000 * 3 + 110_000 * 0.3 + 40_000 * 3.75) / context;
    const tierBlend = (100_000 * 6 + 110_000 * 0.6 + 40_000 * 7.5) / context;
    expect(usd).toBeCloseTo((200_000 * baseBlend + 50_000 * tierBlend) / 1e6, 10);
  });

  it("inherits missing tier fields from the preceding tier, not the base", () => {
    const tiers = [
      { threshold: 128_000, input: 6, output: 22.5, cacheRead: 0.6 },
      { threshold: 256_000, input: 6, output: 30 },
    ];
    const tokens = {
      input: 200_000,
      output: 1_000,
      cacheRead: 100_000,
      cacheWrite: 0,
      reasoning: 0,
    };
    const usd = costUsd(tokens, {
      input: 3,
      output: 15,
      cacheRead: 0.3,
      tiers,
      tierMode: "whole-request",
    });
    expect(usd).toBeCloseTo((200_000 * 6 + 1_000 * 30 + 100_000 * 0.6) / 1e6, 10);
  });

  it("stays on base rates below the threshold in both modes", () => {
    const below = { input: 150_000, output: 5_000, cacheRead: 0, cacheWrite: 0, reasoning: 0 };
    const flat = costUsd(below, { input: 3, output: 15 });
    expect(costUsd(below, { input: 3, output: 15, tiers: [sonnetTier] })).toBeCloseTo(flat, 12);
    expect(
      costUsd(below, { input: 3, output: 15, tiers: [sonnetTier], tierMode: "whole-request" }),
    ).toBeCloseTo(flat, 12);
  });

  // composition of the 1h cache-write split with tiers:
  // a request that both crosses a context tier and carries a 1h cache-write
  // portion must bill the 1h portion at the selected level's 1h rate
  const splitAndTierTokens = {
    input: 100_000,
    output: 5_000,
    cacheRead: 60_000,
    cacheWrite: 90_000,
    cacheWrite1h: 30_000,
    reasoning: 0,
  };
  const splitTier = {
    threshold: 200_000,
    input: 6,
    output: 22.5,
    cacheRead: 0.6,
    cacheWrite: 7.5,
    cacheWrite1h: 12,
  };

  it("composes the 1h cache-write split with a whole-request tier above 200k", () => {
    // context = 100k + 60k + 90k = 250k > 200k, so the tier reprices everything;
    // 60k of the writes are 5m (at 7.5) and 30k are 1h (at the tier 1h rate 12)
    const usd = costUsd(splitAndTierTokens, {
      input: 3,
      output: 15,
      cacheRead: 0.3,
      cacheWrite: 3.75,
      cacheWrite1h: 6,
      tiers: [splitTier],
      tierMode: "whole-request",
    });
    const expected = (100_000 * 6 + 5_000 * 22.5 + 60_000 * 0.6 + 60_000 * 7.5 + 30_000 * 12) / 1e6;
    expect(usd).toBeCloseTo(expected, 10);
  });

  it("composes the 1h cache-write split with a marginal tier above 200k", () => {
    const usd = costUsd(splitAndTierTokens, {
      input: 3,
      output: 15,
      cacheRead: 0.3,
      cacheWrite: 3.75,
      cacheWrite1h: 6,
      tiers: [splitTier],
    });
    const context = 250_000;
    // cache-write cost at each level splits 60k @ 5m rate + 30k @ 1h rate
    const baseBlend = (100_000 * 3 + 60_000 * 0.3 + 60_000 * 3.75 + 30_000 * 6) / context;
    const tierBlend = (100_000 * 6 + 60_000 * 0.6 + 60_000 * 7.5 + 30_000 * 12) / context;
    const expected = (200_000 * baseBlend + 50_000 * tierBlend + 5_000 * 15) / 1e6;
    expect(usd).toBeCloseTo(expected, 10);
  });

  it("defaults the 1h rate to the tier input x 2 when the tier omits it", () => {
    const usd = costUsd(splitAndTierTokens, {
      input: 3,
      output: 15,
      cacheRead: 0.3,
      cacheWrite: 3.75,
      tiers: [{ threshold: 200_000, input: 6, output: 22.5, cacheRead: 0.6, cacheWrite: 7.5 }],
      tierMode: "whole-request",
    });
    // no cacheWrite1h anywhere, so the 1h portion bills at the tier input x 2 = 12
    const expected = (100_000 * 6 + 5_000 * 22.5 + 60_000 * 0.6 + 60_000 * 7.5 + 30_000 * 12) / 1e6;
    expect(usd).toBeCloseTo(expected, 10);
  });
});

describe("source parsing", () => {
  it("parses OpenRouter per-token strings into USD per million", async () => {
    const dir = tempDir();
    try {
      const { fetch } = fetchStub({ "https://or.test/models": OPENROUTER_PAYLOAD });
      const catalog = await openRouterSource("https://or.test/models").load({
        fetch,
        cacheDir: dir,
        offline: false,
        ttlMs: 1000,
      });
      expect(catalog?.prices.get("openai/gpt-5")).toEqual({
        input: 1.25,
        output: 10,
        cacheRead: 0.125,
      });
      expect(catalog?.prices.has("broken")).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("parses LiteLLM and skips sample_spec", async () => {
    const dir = tempDir();
    try {
      const { fetch } = fetchStub({ "https://ll.test/prices.json": LITELLM_PAYLOAD });
      const catalog = await liteLlmSource("https://ll.test/prices.json").load({
        fetch,
        cacheDir: dir,
        offline: false,
        ttlMs: 1000,
      });
      expect(catalog?.prices.get("claude-sonnet-4-5")).toEqual({
        input: 3,
        output: 15,
        cacheRead: 0.3,
        cacheWrite: 3.75,
        cacheWrite1h: 6,
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
        tierMode: "whole-request",
      });
      expect(catalog?.prices.get("bad-1hr-rate")).toEqual({ input: 1, output: 2 });
      expect(catalog?.prices.get("gemini-tiered")).toEqual({
        input: 1.25,
        output: 10,
        tiers: [{ threshold: 128_000, input: 2.5, output: 20 }],
      });
      expect(catalog?.prices.get("cache-only-tiered")).toEqual({
        input: 1,
        output: 5,
        tiers: [{ threshold: 256_000, input: 1, output: 5, cacheRead: 0.5 }],
      });
      expect(catalog?.prices.get("multi-tiered")).toEqual({
        input: 1,
        output: 5,
        tiers: [
          { threshold: 128_000, input: 2, output: 5, cacheRead: 0.25 },
          { threshold: 256_000, input: 2, output: 10, cacheRead: 0.25 },
        ],
      });
      expect(catalog?.prices.get("wide-tiered")).toEqual({
        input: 2,
        output: 10,
        tiers: [
          { threshold: 272_000, input: 4, output: 10, cacheWrite: 5 },
          { threshold: 512_000, input: 4, output: 20, cacheRead: 0.5, cacheWrite: 5 },
        ],
      });
      expect(catalog?.prices.get("gpt-5")).toEqual({ input: 1.25, output: 10 });
      expect(catalog?.prices.has("sample_spec")).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("parses models.dev with provider-qualified and bare keys", async () => {
    const dir = tempDir();
    try {
      const { fetch } = fetchStub({ "https://md.test/api.json": MODELS_DEV_PAYLOAD });
      const catalog = await modelsDevSource("https://md.test/api.json").load({
        fetch,
        cacheDir: dir,
        offline: false,
        ttlMs: 1000,
      });
      expect(catalog?.prices.get("moonshotai/kimi-k3")).toEqual({
        input: 0.6,
        output: 2.5,
        cacheRead: 0.15,
      });
      expect(catalog?.prices.get("kimi-k3")).toBeDefined();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("cache behavior", () => {
  it("serves fresh cache without fetching", async () => {
    const dir = tempDir();
    try {
      const stub = fetchStub({ "https://or.test/models": OPENROUTER_PAYLOAD });
      const source = openRouterSource("https://or.test/models");
      const context = { fetch: stub.fetch, cacheDir: dir, offline: false, ttlMs: 60_000 };
      await source.load(context);
      expect(stub.calls).toHaveLength(1);
      const again = await source.load(context);
      expect(stub.calls).toHaveLength(1);
      expect(again?.prices.get("openai/gpt-5")).toBeDefined();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("refetches after TTL expiry", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", OPENROUTER_PAYLOAD, Date.now() - 120_000);
      const stub = fetchStub({ "https://or.test/models": OPENROUTER_PAYLOAD });
      await openRouterSource("https://or.test/models").load({
        fetch: stub.fetch,
        cacheDir: dir,
        offline: false,
        ttlMs: 60_000,
      });
      expect(stub.calls).toHaveLength(1);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("falls back to any-age stale cache when the fetch fails", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", OPENROUTER_PAYLOAD, Date.now() - 86_400_000);
      const catalog = await openRouterSource("https://or.test/models").load({
        fetch: failingFetch,
        cacheDir: dir,
        offline: false,
        ttlMs: 60_000,
      });
      expect(catalog?.prices.get("openai/gpt-5")).toBeDefined();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("never fetches in offline mode and uses stale cache", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", OPENROUTER_PAYLOAD, Date.now() - 86_400_000);
      const stub = fetchStub({ "https://or.test/models": OPENROUTER_PAYLOAD });
      const catalog = await openRouterSource("https://or.test/models").load({
        fetch: stub.fetch,
        cacheDir: dir,
        offline: true,
        ttlMs: 60_000,
      });
      expect(stub.calls).toHaveLength(0);
      expect(catalog?.prices.get("openai/gpt-5")).toBeDefined();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("refresh forces one refetch past a fresh cache, then reuses the catalogs", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", OPENROUTER_PAYLOAD, Date.now());
      const stub = fetchStub({ "https://or.test/models": OPENROUTER_PAYLOAD });
      const pricing = createPricing({
        sources: [openRouterSource("https://or.test/models")],
        cacheDir: dir,
        fetch: stub.fetch,
        refresh: true,
      });
      await pricing.lookupModel("openai/gpt-5");
      await pricing.lookupModel("anthropic/claude-sonnet-4.5");
      expect(stub.calls).toHaveLength(1);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("refresh is one-shot: TTL-expiry reloads use normal cache rules again", async () => {
    const dir = tempDir();
    try {
      const refreshFlags: (boolean | undefined)[] = [];
      const pricing = createPricing({
        sources: [
          {
            name: "spy",
            load(context) {
              refreshFlags.push(context.refresh);
              return Promise.resolve(null);
            },
          },
        ],
        cacheDir: dir,
        refresh: true,
        ttlMs: 0,
      });
      await pricing.lookupModel("x");
      // ttlMs 0 expires the in-memory catalogs immediately, forcing a reload
      await pricing.lookupModel("x");
      expect(refreshFlags).toEqual([true, false]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("restores the pending refresh when a refresh load rejects", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", OPENROUTER_PAYLOAD, Date.now());
      let failures = 1;
      const stub = fetchStub({ "https://or.test/models": OPENROUTER_PAYLOAD });
      const pricing = createPricing({
        sources: [
          {
            name: "flaky",
            load(context) {
              if (failures > 0) {
                failures -= 1;
                return Promise.reject(new Error("source blew up"));
              }
              return openRouterSource("https://or.test/models").load(context);
            },
          },
        ],
        cacheDir: dir,
        fetch: stub.fetch,
        refresh: true,
      });
      await expect(pricing.lookupModel("openai/gpt-5")).rejects.toThrow();
      // the retry still honours the requested refresh: it fetches past the
      // fresh disk cache instead of serving it
      const retried = await pricing.lookupModel("openai/gpt-5");
      expect(retried.priced).toBe(true);
      expect(stub.calls).toHaveLength(1);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("does not refetch already-refreshed sources when a later source rejects", async () => {
    const dir = tempDir();
    try {
      const refreshFlags: { source: string; refresh: boolean | undefined }[] = [];
      let failures = 1;
      const spySource = (name: string, fail: boolean): PricingSource => ({
        name,
        load(context) {
          refreshFlags.push({ source: name, refresh: context.refresh });
          if (fail && failures > 0) {
            failures -= 1;
            return Promise.reject(new Error(`${name} blew up`));
          }
          return Promise.resolve(null);
        },
      });
      const pricing = createPricing({
        sources: [spySource("first", false), spySource("second", true)],
        cacheDir: dir,
        refresh: true,
      });
      await expect(pricing.lookupModel("x")).rejects.toThrow();
      await pricing.lookupModel("x");
      // the retry must not refresh "first" a second time; "second" never
      // completed its refresh, so it still gets refresh: true
      expect(refreshFlags).toEqual([
        { source: "first", refresh: true },
        { source: "second", refresh: true },
        { source: "first", refresh: false },
        { source: "second", refresh: true },
      ]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("refresh still falls back to the stale cache when the refetch fails", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", OPENROUTER_PAYLOAD, Date.now());
      const catalog = await openRouterSource("https://or.test/models").load({
        fetch: failingFetch,
        cacheDir: dir,
        offline: false,
        ttlMs: 60_000,
        refresh: true,
      });
      expect(catalog?.prices.get("openai/gpt-5")).toBeDefined();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("catalogs() reports source provenance including empty market coverage", async () => {
    const dir = tempDir();
    try {
      const pricing = createPricing({
        sources: [openRouterSource("https://or.test/models")],
        overrides: [{ model: "custom-model", input: 1, output: 2 }],
        cacheDir: dir,
        fetch: failingFetch,
      });
      const catalogs = await pricing.catalogs();
      expect(catalogs).toEqual([{ source: "override", fetchedAt: null, models: 1 }]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("returns null offline with no cache", async () => {
    const dir = tempDir();
    try {
      const catalog = await openRouterSource("https://or.test/models").load({
        fetch: failingFetch,
        cacheDir: dir,
        offline: true,
        ttlMs: 60_000,
      });
      expect(catalog).toBeNull();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("writes cache atomically leaving no tmp files", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", OPENROUTER_PAYLOAD);
      expect(readdirSync(dir).filter((name) => name.endsWith(".tmp"))).toEqual([]);
      const cached = loadCached(dir, "pricing-openrouter.json", 60_000);
      expect(cached?.stale).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("treats a cache payload that parses to zero prices as unusable", async () => {
    const dir = tempDir();
    try {
      storeCached(dir, "pricing-openrouter.json", { data: [] });
      const offline = await openRouterSource("https://or.test/models").load({
        fetch: failingFetch,
        cacheDir: dir,
        offline: true,
        ttlMs: 60_000,
      });
      expect(offline).toBeNull();
      const stub = fetchStub({ "https://or.test/models": OPENROUTER_PAYLOAD });
      const refetched = await openRouterSource("https://or.test/models").load({
        fetch: stub.fetch,
        cacheDir: dir,
        offline: false,
        ttlMs: 60_000,
      });
      expect(stub.calls).toHaveLength(1);
      expect(refetched?.prices.get("openai/gpt-5")).toBeDefined();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("ignores corrupt cache files", () => {
    const dir = tempDir();
    try {
      writeFileSync(join(dir, "pricing-openrouter.json"), "{corrupt");
      expect(loadCached(dir, "pricing-openrouter.json", 60_000)).toBeNull();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("createPricing", () => {
  const routes = {
    "https://or.test/models": OPENROUTER_PAYLOAD,
    "https://ll.test/prices.json": LITELLM_PAYLOAD,
    "https://md.test/api.json": MODELS_DEV_PAYLOAD,
  };

  function pricing(dir: string, overrides?: Parameters<typeof createPricing>[0]) {
    const { fetch } = fetchStub(routes);
    return createPricing({
      sources: [
        openRouterSource("https://or.test/models"),
        liteLlmSource("https://ll.test/prices.json"),
        modelsDevSource("https://md.test/api.json"),
      ],
      cacheDir: dir,
      fetch,
      ...overrides,
    });
  }

  it("prices via the highest-priority source that matches", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir).lookupModel("openai/gpt-5");
      expect(lookup).toMatchObject({ priced: true, source: "openrouter", key: "openai/gpt-5" });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("prefers a tier-bearing LiteLLM match over a flat OpenRouter match", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir).lookupModel("anthropic/claude-sonnet-4.5");
      expect(lookup).toMatchObject({
        priced: true,
        source: "litellm",
        key: "claude-sonnet-4-5",
      });
      if (lookup.priced) {
        expect(lookup.price.tierMode).toBe("whole-request");
        expect(lookup.price.tiers).toHaveLength(1);
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("does not let a lower-priority source named 'override' hijack priority", async () => {
    const dir = tempDir();
    try {
      const { fetch } = fetchStub(routes);
      const impostor: PricingSource = {
        name: "override",
        load: async () => ({
          source: "override",
          fetchedAt: null,
          prices: new Map([["openai/gpt-5", { input: 999, output: 999 }]]),
        }),
      };
      const p = createPricing({
        sources: [openRouterSource("https://or.test/models"), impostor],
        cacheDir: dir,
        fetch,
      });
      const lookup = await p.lookupModel("openai/gpt-5");
      expect(lookup).toMatchObject({ priced: true, source: "openrouter" });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("falls through to lower-priority sources", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir).lookupModel("kimi-k3");
      expect(lookup).toMatchObject({ priced: true, source: "models-dev" });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("lets overrides beat every market source", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir, {
        overrides: [{ model: "openai/gpt-5", input: 1, output: 2 }],
      }).lookupModel("openai/gpt-5");
      expect(lookup).toMatchObject({ priced: true, source: "override", fetchedAt: null });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("matches through the tiers (prefix strip against LiteLLM keys)", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir).lookupModel("someprovider/gpt-5");
      expect(lookup).toMatchObject({ priced: true, key: "gpt-5" });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("prices Bedrock claude-family ids at the claude rate, not the zero placeholder", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir).lookupModel("us.anthropic.claude-opus-4-6-20260115-v1:0");
      expect(lookup).toMatchObject({
        priced: true,
        source: "openrouter",
        key: "anthropic/claude-opus-4.6",
        price: { input: 5, output: 25 },
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("falls through a higher-priority zero-cost match to a lower-priority real rate", async () => {
    const dir = tempDir();
    try {
      const { fetch } = fetchStub(routes);
      const instance = createPricing({
        sources: [
          modelsDevSource("https://md.test/api.json"),
          liteLlmSource("https://ll.test/prices.json"),
        ],
        cacheDir: dir,
        fetch,
      });
      const lookup = await instance.lookupModel("us.anthropic.claude-opus-4-6-20260115-v1:0");
      expect(lookup).toMatchObject({
        priced: true,
        source: "litellm",
        key: "claude-opus-4.6",
        price: { input: 5, output: 25 },
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("treats a model that only exists as a zero-cost entry as unpriced with reason", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir).lookupModel("amazon-bedrock/anthropic.mystery-9-v1:0");
      expect(lookup).toMatchObject({
        priced: false,
        reason: "zeroCost",
        key: "amazon-bedrock/anthropic.mystery-9-v1:0",
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("still skips zero-cost entries from a user source named override", async () => {
    const dir = tempDir();
    try {
      const instance = createPricing({
        sources: [
          {
            name: "override",
            load: () =>
              Promise.resolve({
                source: "override",
                fetchedAt: null,
                prices: new Map([["freebie", { input: 0, output: 0 }]]),
              }),
          },
        ],
        cacheDir: dir,
      });
      const lookup = await instance.lookupModel("freebie");
      expect(lookup).toMatchObject({ priced: false, reason: "zeroCost", key: "freebie" });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("keeps an explicit custom $0 override priced", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir, {
        overrides: [{ model: "amazon-bedrock/anthropic.mystery-9-v1:0", input: 0, output: 0 }],
      }).lookupModel("amazon-bedrock/anthropic.mystery-9-v1:0");
      expect(lookup).toMatchObject({
        priced: true,
        source: "override",
        price: { input: 0, output: 0 },
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("reports honest misses with attempted keys", async () => {
    const dir = tempDir();
    try {
      const lookup = await pricing(dir).lookupModel("total-mystery");
      expect(lookup.priced).toBe(false);
      if (!lookup.priced) expect(lookup.attempted).toContain("total-mystery");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("reconsults sources after the TTL window on a long-lived instance", async () => {
    const dir = tempDir();
    try {
      const stub = fetchStub(routes);
      const instance = createPricing({
        sources: [openRouterSource("https://or.test/models")],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: 0,
      });
      await instance.lookupModel("openai/gpt-5");
      const first = stub.calls.length;
      await new Promise((resolve) => setTimeout(resolve, 5));
      await instance.lookupModel("openai/gpt-5");
      expect(stub.calls.length).toBeGreaterThan(first);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("retries after a rejected catalog load instead of memoizing the rejection", async () => {
    const dir = tempDir();
    try {
      let failures = 1;
      const flaky: FetchLike = (url) => {
        if (failures > 0) {
          failures -= 1;
          throw new Error("synchronous fetch explosion");
        }
        return fetchStub(routes).fetch(url);
      };
      const instance = createPricing({
        sources: [
          {
            name: "explosive",
            load: () => {
              if (failures > 0) {
                failures -= 1;
                return Promise.reject(new Error("source blew up"));
              }
              return openRouterSource("https://or.test/models").load({
                fetch: fetchStub(routes).fetch,
                cacheDir: dir,
                offline: false,
                ttlMs: 60_000,
              });
            },
          },
        ],
        cacheDir: dir,
        fetch: flaky,
      });
      await expect(instance.lookupModel("openai/gpt-5")).rejects.toThrow();
      const retried = await instance.lookupModel("openai/gpt-5");
      expect(retried.priced).toBe(true);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("prices rollups and computes USD", async () => {
    const dir = tempDir();
    try {
      const priced = await pricing(dir).priceRollups([
        {
          key: "openai/gpt-5",
          tokens: { ...zeroTokens, input: 1_000_000, output: 100_000 },
          events: 1,
          turns: 1,
          calls: 1,
        },
        {
          key: "total-mystery",
          tokens: { ...zeroTokens, input: 5 },
          events: 1,
          turns: 0,
          calls: 1,
        },
      ]);
      expect(priced[0]?.pricing).toMatchObject({ priced: true, usd: 1.25 + 1 });
      expect(priced[1]?.pricing.priced).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  const recordedRollup = {
    key: "openai/gpt-5",
    tokens: { ...zeroTokens, input: 1_000_000, output: 100_000 },
    events: 1,
    turns: 1,
    calls: 1,
    costUsd: 0.5,
  };

  it("mode auto returns the recorded cost and calculate returns the token-derived cost", async () => {
    const dir = tempDir();
    try {
      const auto = await pricing(dir, { mode: "auto" }).priceRollups([recordedRollup]);
      expect(auto[0]?.pricing).toMatchObject({ priced: true, usd: 0.5 });
      const calculate = await pricing(dir).priceRollups([recordedRollup]);
      expect(calculate[0]?.pricing).toMatchObject({ priced: true, usd: 1.25 + 1 });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("events without recorded cost behave identically in auto and calculate", async () => {
    const dir = tempDir();
    try {
      const { costUsd: _omit, ...noRecorded } = recordedRollup;
      const auto = await pricing(dir, { mode: "auto" }).priceRollups([noRecorded]);
      const calculate = await pricing(dir).priceRollups([noRecorded]);
      expect(auto[0]?.pricing).toEqual(calculate[0]?.pricing);
      expect(auto[0]?.pricing).toMatchObject({ usd: 1.25 + 1 });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("mode display reports the recorded cost or zero, ignoring token math", async () => {
    const dir = tempDir();
    try {
      const { costUsd: _omit, ...noRecorded } = recordedRollup;
      const priced = await pricing(dir, { mode: "display" }).priceRollups([
        recordedRollup,
        noRecorded,
      ]);
      expect(priced[0]?.pricing.usd).toBe(0.5);
      expect(priced[1]?.pricing.usd).toBe(0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("mode auto falls back to the recorded cost even when the model is unpriced", async () => {
    const dir = tempDir();
    try {
      const priced = await pricing(dir, { mode: "auto" }).priceRollups([
        { ...recordedRollup, key: "total-mystery" },
      ]);
      expect(priced[0]?.pricing.priced).toBe(false);
      expect(priced[0]?.pricing.usd).toBe(0.5);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("mode auto honors a recorded zero instead of falling back to token math", async () => {
    const dir = tempDir();
    try {
      const priced = await pricing(dir, { mode: "auto" }).priceRollups([
        { ...recordedRollup, costUsd: 0 },
        { ...recordedRollup, key: "total-mystery", costUsd: 0 },
      ]);
      expect(priced[0]?.pricing).toMatchObject({ priced: true, usd: 0 });
      expect(priced[1]?.pricing).toMatchObject({ priced: false, usd: 0 });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("reads the claude fixture: auto returns the recorded cost, calculate the token-derived one", async () => {
    const dir = tempDir();
    try {
      const fixtures = join(dirname(fileURLToPath(import.meta.url)), "fixtures");
      const { events } = await readUsage({
        home: fixtures,
        env: { CLAUDE_CONFIG_DIR: join(fixtures, "claude") },
        harnesses: ["claude"],
      });
      const recorded = events.filter((event) => event.messageId === "msg_a2");
      const rollups = rollup(recorded, { by: "session" });
      rollups[0] = { ...rollups[0]!, key: "claude-sonnet-4-5-20250929" };
      const auto = await pricing(dir, { mode: "auto" }).priceRollups(rollups);
      expect(auto[0]?.pricing).toMatchObject({ priced: true, usd: 0.0123 });
      const calculate = await pricing(dir).priceRollups(rollups);
      expect(calculate[0]?.pricing).toMatchObject({
        priced: true,
        usd: (10 * 3 + 20 * 15 + 600 * 0.3) / 1_000_000,
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("priceEvents", () => {
  const routes = {
    "https://or.test/models": OPENROUTER_PAYLOAD,
    "https://ll.test/prices.json": LITELLM_PAYLOAD,
    "https://md.test/api.json": MODELS_DEV_PAYLOAD,
  };

  function pricing(dir: string, overrides?: Parameters<typeof createPricing>[0]) {
    const { fetch } = fetchStub(routes);
    return createPricing({
      sources: [
        openRouterSource("https://or.test/models"),
        liteLlmSource("https://ll.test/prices.json"),
        modelsDevSource("https://md.test/api.json"),
      ],
      cacheDir: dir,
      fetch,
      ...overrides,
    });
  }

  function claudeEvent(input: number, sessionId: string): UsageEvent {
    return {
      harness: "claude",
      timestamp: "2026-08-01T00:00:00.000Z",
      sessionId,
      messageId: `${sessionId}-msg`,
      turn: true,
      subagent: false,
      model: "claude-sonnet-4-5-20250929",
      tokens: { ...zeroTokens, input },
    };
  }

  it("prices two 150k events per-event without crossing the 200k tier", async () => {
    const dir = tempDir();
    try {
      const events = [claudeEvent(150_000, "s1"), claudeEvent(150_000, "s2")];
      const perEvent = await pricing(dir).priceEvents(events, { by: "model" });
      const aggregate = await pricing(dir).priceRollups(rollup(events, { by: "model" }));
      const perEventUsd = perEvent[0]?.pricing.usd;
      const aggregateUsd = aggregate[0]?.pricing.usd;
      expect(perEventUsd).toBeCloseTo((300_000 * 3) / 1_000_000, 10);
      expect(aggregateUsd).toBeCloseTo((300_000 * 6) / 1_000_000, 10);
      expect(perEventUsd).toBeLessThan(aggregateUsd ?? Number.POSITIVE_INFINITY);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("flags the aggregate rollup path as a tiered mispricing exposure", async () => {
    const dir = tempDir();
    try {
      const events = [claudeEvent(150_000, "s1"), claudeEvent(150_000, "s2")];
      const aggregate = await pricing(dir).priceRollups(rollup(events, { by: "model" }));
      expect(aggregate[0]?.pricing).toMatchObject({ priced: true, tieredAggregate: true });
      const perEvent = await pricing(dir).priceEvents(events, { by: "model" });
      expect(perEvent[0]?.pricing).not.toHaveProperty("tieredAggregate");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("does not flag a single-event tiered rollup", async () => {
    const dir = tempDir();
    try {
      const aggregate = await pricing(dir).priceRollups(
        rollup([claudeEvent(150_000, "s1")], { by: "model" }),
      );
      expect(aggregate[0]?.pricing).not.toHaveProperty("tieredAggregate");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("aggregates per-event prices into groups matching rollup keys", async () => {
    const dir = tempDir();
    try {
      const events = [claudeEvent(150_000, "s1"), claudeEvent(150_000, "s2")];
      const perEvent = await pricing(dir).priceEvents(events, { by: "session" });
      expect(perEvent).toHaveLength(2);
      expect(perEvent.map((r) => r.key)).toEqual(["s1", "s2"]);
      for (const row of perEvent) {
        expect(row.pricing.usd).toBeCloseTo((150_000 * 3) / 1_000_000, 10);
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("reports a group unpriced when any member model is unpriced", async () => {
    const dir = tempDir();
    try {
      const events = [
        claudeEvent(1_000, "s1"),
        { ...claudeEvent(1_000, "s1"), model: "total-mystery", messageId: "s1-msg2" },
      ];
      const perEvent = await pricing(dir).priceEvents(events, { by: "session" });
      expect(perEvent[0]?.pricing.priced).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("mode display and auto honor recorded cost over per-event token math", async () => {
    const dir = tempDir();
    try {
      const events = [{ ...claudeEvent(1_000_000, "s1"), costUsd: 0.5 }];
      const display = await pricing(dir, { mode: "display" }).priceEvents(events, {
        by: "session",
      });
      expect(display[0]?.pricing.usd).toBe(0.5);
      const auto = await pricing(dir, { mode: "auto" }).priceEvents(events, { by: "session" });
      expect(auto[0]?.pricing.usd).toBe(0.5);
      const calculate = await pricing(dir).priceEvents(events, { by: "session" });
      expect(calculate[0]?.pricing.usd).toBeCloseTo((1_000_000 * 6) / 1_000_000, 10);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("mode auto applies recorded-or-calculated per event, not per group", async () => {
    const dir = tempDir();
    try {
      const events = [
        { ...claudeEvent(1_000, "s1"), costUsd: 0.5 },
        { ...claudeEvent(2_000, "s1"), messageId: "s1-msg2" },
      ];
      const auto = await pricing(dir, { mode: "auto" }).priceEvents(events, { by: "session" });
      expect(auto[0]?.pricing.usd).toBeCloseTo(0.5 + (2_000 * 3) / 1_000_000, 10);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("reports an aggregate hit with the model set for a mixed-model group", async () => {
    const dir = tempDir();
    try {
      const events = [
        claudeEvent(1_000, "s1"),
        { ...claudeEvent(1_000, "s1"), model: "openai/gpt-5", messageId: "s1-msg2" },
      ];
      const perEvent = await pricing(dir).priceEvents(events, { by: "session" });
      const pricingResult = perEvent[0]?.pricing;
      expect(pricingResult?.priced).toBe(true);
      expect(pricingResult).not.toHaveProperty("price");
      expect(pricingResult).not.toHaveProperty("source");
      if (pricingResult !== undefined && "models" in pricingResult) {
        expect(pricingResult.models).toEqual(["claude-sonnet-4-5-20250929", "openai/gpt-5"]);
      }
      const expected = (1_000 * 3 + (1_000 * 1.25 + 0)) / 1_000_000;
      expect(pricingResult?.usd).toBeCloseTo(expected, 10);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("keeps single-model group provenance intact", async () => {
    const dir = tempDir();
    try {
      const perEvent = await pricing(dir).priceEvents([claudeEvent(1_000, "s1")], {
        by: "session",
      });
      const pricingResult = perEvent[0]?.pricing;
      expect(pricingResult).toMatchObject({
        priced: true,
        source: "litellm",
        key: "claude-sonnet-4-5",
        model: "claude-sonnet-4-5-20250929",
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("preserves a zero-cost miss reason for a single-model group", async () => {
    const dir = tempDir();
    try {
      const events = [
        {
          ...claudeEvent(1_000, "s1"),
          model: "amazon-bedrock/anthropic.mystery-9-v1:0",
          messageId: "s1-m1",
        },
        {
          ...claudeEvent(1_000, "s1"),
          model: "amazon-bedrock/anthropic.mystery-9-v1:0",
          messageId: "s1-m2",
        },
      ];
      const perEvent = await pricing(dir).priceEvents(events, { by: "session" });
      expect(perEvent[0]?.pricing).toMatchObject({
        priced: false,
        reason: "zeroCost",
        key: "amazon-bedrock/anthropic.mystery-9-v1:0",
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("flags a tiered multi-call event as an aggregate exposure on both paths", async () => {
    const dir = tempDir();
    try {
      const event = { ...claudeEvent(150_000, "s1"), calls: 3 };
      const perEvent = await pricing(dir).priceEvents([event], { by: "model" });
      expect(perEvent[0]?.pricing).toMatchObject({ priced: true, tieredAggregate: true });
      const aggregate = await pricing(dir).priceRollups(rollup([event], { by: "model" }));
      expect(aggregate[0]?.pricing).toMatchObject({ priced: true, tieredAggregate: true });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("lifecycle conformance (shared fixtures)", () => {
  const lifecycleDir = join(
    dirname(fileURLToPath(import.meta.url)),
    "..",
    "golden",
    "pricing",
    "lifecycle",
  );

  function loadFixture<T>(...segments: string[]): T {
    return JSON.parse(readFileSync(join(lifecycleDir, ...segments), "utf8")) as T;
  }

  type Request = {
    source: string;
    cacheFileName: string;
    ttlMs: number;
    now: string;
    fetchedAtFresh: string;
    fetchedAtStale: string;
    fetchedAtFetched: string;
    nowSecond: string;
    options: { mode: PricingMode };
    rollup: Rollup;
  };
  type Expected = {
    behavior: string;
    fetched: boolean;
    fetchedAt: string | null;
    priced: boolean;
    pricedUsd: number;
  };
  type ExpectedReload = {
    behavior: string;
    fetchesTotal: number;
    fetchedAtFirst: string;
    fetchedAtSecond: string;
    priced: boolean;
    pricedUsd: number;
  };

  // load an expected-outcome fixture and consume its `behavior` field, so every
  // field of each expected/*.json is asserted by at least one behavior
  function loadExpected(behavior: string): Expected {
    const expected = loadFixture<Expected>("expected", `${behavior}.json`);
    expect(expected.behavior).toBe(behavior);
    return expected;
  }

  const request = loadFixture<Request>("request.json");
  const freshCache = loadFixture<{ fetchedAt: string; payload: unknown }>("cache-fresh.json");
  const staleCache = loadFixture<{ fetchedAt: string; payload: unknown }>("cache-stale.json");
  const sourcePayload = freshCache.payload;

  const SOURCE_URL = "https://or.test/models";

  // the fixtures pin a fixed clock and exact resulting timestamps; fake the
  // system clock to request.now so `Date.now()`/`new Date()` (in cache.ts and
  // index.ts) are deterministic and a live fetch stamps exactly fetchedAtFetched
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(request.now));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  // a recording fetch stub: counts calls, then serves the payload (or, when
  // `throws`, rejects after incrementing so the attempt is observable)
  function recordingFetch(payload: unknown, throws = false): { fetch: FetchLike; calls: number } {
    const state = { calls: 0, fetch: (() => Promise.resolve()) as unknown as FetchLike };
    state.fetch = (() => {
      state.calls += 1;
      if (throws) return Promise.reject(new Error("network down"));
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(payload) });
    }) as FetchLike;
    return state;
  }

  function installCache(dir: string, cache: { fetchedAt: string; payload: unknown }): void {
    // install the committed fixture verbatim (byte-compatible consumption)
    writeFileSync(join(dir, request.cacheFileName), JSON.stringify(cache));
  }

  function currentFetchedAt(dir: string): string {
    const raw = JSON.parse(readFileSync(join(dir, request.cacheFileName), "utf8")) as {
      fetchedAt: string;
    };
    return raw.fetchedAt;
  }

  it("cold-fetch: empty cacheDir fetches, writes cache with fetchedAt=now, prices to gold", async () => {
    const expected = loadExpected("cold-fetch");
    const dir = tempDir();
    try {
      const stub = recordingFetch(sourcePayload);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      expect(pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      // a live fetch stamps the file with the faked clock: exactly fetchedAtFetched
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAt);
      expect(currentFetchedAt(dir)).toBe(request.fetchedAtFetched);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("warm-cache: the committed fresh cache serves without fetching, fetchedAt exact", async () => {
    const expected = loadExpected("warm-cache");
    const dir = tempDir();
    try {
      // install cache-fresh.json verbatim; under the faked clock (now one minute
      // past its fetchedAt, ttl 60s) it is fresh, so no fetch is issued
      installCache(dir, freshCache);
      const stub = recordingFetch(sourcePayload);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      expect(pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAt);
      expect(currentFetchedAt(dir)).toBe(request.fetchedAtFresh);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("ttl-refresh: a stale cache refetches once and advances fetchedAt to now", async () => {
    const expected = loadExpected("ttl-refresh");
    const dir = tempDir();
    try {
      installCache(dir, staleCache);
      const stub = recordingFetch(sourcePayload);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        refresh: true,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      expect(pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAt);
      expect(currentFetchedAt(dir)).toBe(request.fetchedAtFetched);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("offline: a present cache serves without fetching, fetchedAt untouched", async () => {
    const expected = loadExpected("offline");
    const dir = tempDir();
    try {
      installCache(dir, staleCache);
      const stub = recordingFetch(sourcePayload);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        offline: true,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      expect(pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAt);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("offline-no-cache: an empty cacheDir skips the source and degrades to a miss", async () => {
    const expected = loadExpected("offline-no-cache");
    const dir = tempDir();
    try {
      const stub = recordingFetch(sourcePayload);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        offline: true,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      // no fetch, no crash, an unpriced miss
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      // fetchedAt: null => no cache file was written
      expect(expected.fetchedAt).toBeNull();
      expect(existsSync(join(dir, request.cacheFileName))).toBe(false);
      // pricedUsd: 0 => the miss carries no USD value
      expect(expected.pricedUsd).toBe(0);
      expect(pricing?.usd ?? 0).toBe(expected.pricedUsd);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("stale-fallback: a failing fetch is attempted once and the stale cache is served", async () => {
    const expected = loadExpected("stale-fallback");
    const dir = tempDir();
    try {
      installCache(dir, staleCache);
      const stub = recordingFetch(sourcePayload, true);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      // the fetch is attempted exactly once (fetched:true) but throws; the stale
      // payload is served rather than erroring, and its fetchedAt is preserved
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      expect(pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAt);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("fetched-empty: a fetch that parses to zero prices leaves the stale cache authoritative", async () => {
    const expected = loadExpected("fetched-empty");
    const emptyPayload = loadFixture<unknown>("source-openrouter-empty.json");
    const dir = tempDir();
    try {
      installCache(dir, staleCache);
      const stub = recordingFetch(emptyPayload);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      // the fetch is attempted once, its unusable payload is discarded: the
      // stale catalog is served and the cache file is NOT overwritten
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      expect(pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAt);
      expect(currentFetchedAt(dir)).toBe(request.fetchedAtStale);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("cached-empty: a fresh cache with zero prices is unusable, so a fetch is issued", async () => {
    const expected = loadExpected("cached-empty");
    const emptyCache = loadFixture<{ fetchedAt: string; payload: unknown }>(
      "cache-fresh-empty.json",
    );
    const dir = tempDir();
    try {
      installCache(dir, emptyCache);
      const stub = recordingFetch(sourcePayload);
      const priced = await createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        ...request.options,
      }).priceRollups([request.rollup]);
      const pricing = priced[0]?.pricing;
      expect(stub.calls).toBe(Number(expected.fetched));
      expect(pricing).toMatchObject({ priced: expected.priced });
      expect(pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAt);
      expect(currentFetchedAt(dir)).toBe(request.fetchedAtFetched);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("live-reload: one instance consults sources again after the TTL window", async () => {
    const expected = loadFixture<ExpectedReload>("expected", "live-reload.json");
    expect(expected.behavior).toBe("live-reload");
    const dir = tempDir();
    try {
      const stub = recordingFetch(sourcePayload);
      const pricing = createPricing({
        sources: [openRouterSource(SOURCE_URL)],
        cacheDir: dir,
        fetch: stub.fetch,
        ttlMs: request.ttlMs,
        ...request.options,
      });
      const first = await pricing.priceRollups([request.rollup]);
      expect(first[0]?.pricing).toMatchObject({ priced: expected.priced });
      expect(first[0]?.pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAtFirst);
      // advance the clock past the TTL window; the same instance must consult
      // sources again, and the now-stale disk cache forces a second fetch
      vi.setSystemTime(new Date(request.nowSecond));
      const second = await pricing.priceRollups([request.rollup]);
      expect(second[0]?.pricing).toMatchObject({ priced: expected.priced });
      expect(second[0]?.pricing?.usd).toBeCloseTo(expected.pricedUsd, 10);
      expect(stub.calls).toBe(expected.fetchesTotal);
      expect(currentFetchedAt(dir)).toBe(expected.fetchedAtSecond);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  // every source fixture parses through its format and prices the
  // shared rollup to pricedUsd (2.25), so all three parser shapes stay valid
  it("source fixtures each parse and price the shared rollup to the gold total", async () => {
    const cases: Array<[string, (url: string) => PricingSource, string]> = [
      ["source-openrouter.json", openRouterSource, "openrouter"],
      ["source-litellm.json", liteLlmSource, "litellm"],
      ["source-modelsdev.json", modelsDevSource, "models-dev"],
    ];
    for (const [file, makeSource, name] of cases) {
      const payload = loadFixture<unknown>(file);
      const dir = tempDir();
      try {
        const stub = recordingFetch(payload);
        const priced = await createPricing({
          sources: [makeSource(`https://${name}.test/api`)],
          cacheDir: dir,
          fetch: stub.fetch,
          ttlMs: request.ttlMs,
          ...request.options,
        }).priceRollups([request.rollup]);
        const pricing = priced[0]?.pricing;
        expect(pricing, name).toMatchObject({ priced: true });
        expect(pricing?.usd, name).toBeCloseTo(2.25, 10);
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    }
  });

  // the shared golden constants contract (constants.json): the facade's own
  // source names, URLs, priority order, TTL default, fetch timeout, and cache
  // filenames must equal the golden values so no facade's literals drift
  it("lifecycle constants match the golden contract", () => {
    type ConstantsSource = {
      name: string;
      format: string;
      url: string;
      cacheFileName: string;
    };
    type Constants = {
      sources: ConstantsSource[];
      priorityOrder: string[];
      defaultTtlMs: number;
      fetchTimeoutMs: number;
      cacheFileNamePattern: string;
    };
    const constants = loadFixture<Constants>("constants.json");

    const urlByName: Record<string, string> = {
      openrouter: OPENROUTER_URL,
      litellm: LITELLM_URL,
      "models-dev": MODELS_DEV_URL,
    };

    // default sources in the TS reference order (OpenRouter > LiteLLM > models.dev)
    const defaultSources = [openRouterSource(), liteLlmSource(), modelsDevSource()];
    expect(defaultSources.map((s) => s.name)).toEqual(constants.priorityOrder);
    expect(constants.priorityOrder).toEqual(constants.sources.map((s) => s.name));

    for (const source of constants.sources) {
      expect(urlByName[source.name], source.name).toBe(source.url);
      expect(BUILTIN_SOURCE_FORMATS[source.name], source.name).toBe(source.format);
      // compare the production filename formula (sources.ts cacheFileName), not
      // just the fixture pattern, so the real formula cannot drift unseen
      expect(cacheFileName(source.name), source.name).toBe(source.cacheFileName);
      expect(source.cacheFileName, source.name).toBe(
        constants.cacheFileNamePattern.replace("{name}", source.name),
      );
    }

    expect(DEFAULT_TTL_MS).toBe(constants.defaultTtlMs);
    expect(FETCH_TIMEOUT_MS).toBe(constants.fetchTimeoutMs);
  });
});

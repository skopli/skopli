import { describe, expect, it } from "vitest";
import { rollup } from "../src/rollup.ts";
import type { UsageEvent } from "../src/types.ts";

function event(overrides: Partial<UsageEvent>): UsageEvent {
  return {
    harness: "claude",
    timestamp: "2026-08-01T10:00:00Z",
    sessionId: "s1",
    messageId: "m1",
    turn: false,
    subagent: false,
    model: "claude-sonnet-4-5",
    tokens: { input: 10, output: 5, cacheRead: 0, cacheWrite: 0, reasoning: 0 },
    ...overrides,
  };
}

describe("rollup", () => {
  it("groups by model with sorted keys", () => {
    const rows = rollup(
      [
        event({ model: "b-model", turn: true }),
        event({ model: "a-model" }),
        event({ model: "b-model" }),
      ],
      { by: "model" },
    );
    expect(rows.map((r) => r.key)).toEqual(["a-model", "b-model"]);
    expect(rows[1]).toMatchObject({ events: 2, turns: 1, calls: 2 });
    expect(rows[1]?.tokens.input).toBe(20);
  });

  it("sums cacheWrite1h and leaves it absent when no event reports it", () => {
    const withSplit = rollup(
      [
        event({
          tokens: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 9,
            cacheWrite1h: 3,
            reasoning: 0,
          },
        }),
        event({
          messageId: "m2",
          tokens: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 5,
            cacheWrite1h: 2,
            reasoning: 0,
          },
        }),
        event({
          messageId: "m3",
          tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 4, reasoning: 0 },
        }),
      ],
      { by: "model" },
    );
    expect(withSplit[0]?.tokens.cacheWrite).toBe(18);
    expect(withSplit[0]?.tokens.cacheWrite1h).toBe(5);
    const without = rollup([event({})], { by: "model" });
    expect(without[0]?.tokens.cacheWrite1h).toBeUndefined();
  });

  it("clamps each event's 1h split so malformed events cannot rebill others' writes", () => {
    const rows = rollup(
      [
        event({
          tokens: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 10,
            cacheWrite1h: 100,
            reasoning: 0,
          },
        }),
        event({
          messageId: "m2",
          tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 100, reasoning: 0 },
        }),
        event({
          messageId: "m3",
          tokens: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 20,
            cacheWrite1h: -5,
            reasoning: 0,
          },
        }),
        event({
          messageId: "m4",
          tokens: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 30,
            cacheWrite1h: Number.NaN,
            reasoning: 0,
          },
        }),
        event({
          messageId: "m5",
          tokens: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: -10,
            cacheWrite1h: 5,
            reasoning: 0,
          },
        }),
      ],
      { by: "model" },
    );
    expect(rows[0]?.tokens.cacheWrite).toBe(150);
    expect(rows[0]?.tokens.cacheWrite1h).toBe(10);
  });

  it("uses reported finite calls and defaults to 1 otherwise", () => {
    const rows = rollup(
      [
        event({ calls: 5 }),
        event({ messageId: "m2" }),
        event({ messageId: "m3", calls: Number.NaN }),
        event({ messageId: "m4", calls: Number.POSITIVE_INFINITY }),
      ],
      { by: "model" },
    );
    expect(rows[0]?.calls).toBe(8);
  });

  it("groups by session and harness", () => {
    const rows = rollup([event({ sessionId: "s2" }), event({})], { by: "session" });
    expect(rows.map((r) => r.key)).toEqual(["s1", "s2"]);
    const byHarness = rollup([event({ harness: "codex" }), event({})], { by: "harness" });
    expect(byHarness.map((r) => r.key)).toEqual(["claude", "codex"]);
  });

  it("buckets days in the requested timezone", () => {
    const nearMidnight = event({ timestamp: "2026-08-01T23:30:00Z" });
    const utc = rollup([nearMidnight], { by: "day", tz: "UTC" });
    expect(utc[0]?.key).toBe("2026-08-01");
    const tokyo = rollup([nearMidnight], { by: "day", tz: "Asia/Tokyo" });
    expect(tokyo[0]?.key).toBe("2026-08-02");
    const la = rollup([event({ timestamp: "2026-08-02T05:00:00Z" })], {
      by: "day",
      tz: "America/Los_Angeles",
    });
    expect(la[0]?.key).toBe("2026-08-01");
  });

  it("buckets unparsable timestamps under invalid-date", () => {
    const rows = rollup([event({ timestamp: "garbage" })], { by: "day", tz: "UTC" });
    expect(rows[0]?.key).toBe("invalid-date");
  });

  it("sums recorded cost only when an event carried one", () => {
    const withCost = rollup(
      [
        event({ costUsd: 0.25 }),
        event({ messageId: "m2", costUsd: 0.75 }),
        event({ messageId: "m3" }),
      ],
      { by: "model" },
    );
    expect(withCost[0]?.costUsd).toBe(1);
    const without = rollup([event({}), event({ messageId: "m2" })], { by: "model" });
    expect(without[0]?.costUsd).toBeUndefined();
  });

  it("keeps an explicit recorded zero distinct from an absent cost", () => {
    const rows = rollup([event({ costUsd: 0 })], { by: "model" });
    expect(rows[0]?.costUsd).toBe(0);
  });

  it("groups by workspace and folds missing workspaces under the (unknown) sentinel", () => {
    const rows = rollup(
      [
        event({ messageId: "m1", workspace: "/repo/a" }),
        event({ messageId: "m2", workspace: "/repo/b" }),
        event({ messageId: "m3", workspace: "/repo/a" }),
        event({ messageId: "m4" }),
      ],
      { by: "workspace" },
    );
    expect(rows.map((r) => r.key)).toEqual(["(unknown)", "/repo/a", "/repo/b"]);
    expect(rows.find((r) => r.key === "/repo/a")).toMatchObject({ events: 2 });
    expect(rows.find((r) => r.key === "(unknown)")).toMatchObject({ events: 1 });
  });

  it("keeps the original four rollup keys unchanged when workspace fields are present", () => {
    const events = [
      event({ messageId: "m1", workspace: "/repo/a", title: "t1" }),
      event({ messageId: "m2", model: "other", workspace: "/repo/b", title: "t2" }),
    ];
    const bare = [event({ messageId: "m1" }), event({ messageId: "m2", model: "other" })];
    for (const by of ["model", "day", "session", "harness"] as const) {
      expect(rollup(events, { by })).toEqual(rollup(bare, { by }));
    }
  });
});

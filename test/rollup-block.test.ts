import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { rollup } from "../src/rollup.ts";
import type { UsageEvent } from "../src/types.ts";

// Billing-block windowing conformance, driven by the SHARED fixtures in
// golden/rollup-block/. The Rust core and every language facade consume the same
// gold, so TS and the native core must agree on every block anchor and split.

type Block = { key: string; events: number };
type Case = { name: string; tz: string; blockMs: number; timestamps: string[]; expected: Block[] };

function event(timestamp: string): UsageEvent {
  return {
    harness: "opencode",
    timestamp,
    sessionId: "s",
    messageId: "m",
    turn: false,
    subagent: false,
    model: "m",
    tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0 },
  };
}

const casesPath = fileURLToPath(new URL("../golden/rollup-block/cases.json", import.meta.url));
const cases = JSON.parse(readFileSync(casesPath, "utf8")) as Case[];

describe("rollup billing blocks (shared gold)", () => {
  for (const kase of cases) {
    it(kase.name, () => {
      const events = kase.timestamps.map(event);
      const rows = rollup(events, { by: "block", tz: kase.tz, blockMs: kase.blockMs });
      expect(rows.map((r) => ({ key: r.key, events: r.events }))).toEqual(kase.expected);
    });
  }
});

// The shared fixture schema always carries blockMs, so omitting it (using the
// five-hour default) is pinned here per suite instead.
describe("rollup billing blocks (omitted blockMs uses the default)", () => {
  it("two events within five hours share one default block", () => {
    const events = [event("2026-01-01T09:17:00.000Z"), event("2026-01-01T13:00:00.000Z")];
    const rows = rollup(events, { by: "block", tz: "UTC" });
    expect(rows.map((r) => ({ key: r.key, events: r.events }))).toEqual([
      { key: "2026-01-01T09:00:00.000Z", events: 2 },
    ]);
  });

  it("an event past the default five hours opens a new block", () => {
    const events = [event("2026-01-01T09:00:00.000Z"), event("2026-01-01T14:30:00.000Z")];
    const rows = rollup(events, { by: "block", tz: "UTC" });
    expect(rows.map((r) => ({ key: r.key, events: r.events }))).toEqual([
      { key: "2026-01-01T09:00:00.000Z", events: 1 },
      { key: "2026-01-01T14:00:00.000Z", events: 1 },
    ]);
  });
});

// An omitted tz must resolve the system zone (matching the Rust core, which
// routes an omitted zone to TimeZone::system()), not fall back to UTC. These
// tests pin that parity so a host with a fractional-hour offset cannot diverge.
describe("rollup billing blocks (omitted tz resolves the system zone)", () => {
  it("omitted tz output equals explicit resolved-system-zone output", () => {
    // Portable: assert equality between the omitted-tz call and one that names
    // the resolved system zone explicitly, rather than pinning any host zone.
    const events = [
      event("2026-01-01T09:17:00.000Z"),
      event("2026-01-01T13:00:00.000Z"),
      event("2026-01-01T20:45:00.000Z"),
    ];
    const systemZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const omitted = rollup(events, { by: "block" });
    const explicit = rollup(events, { by: "block", tz: systemZone });
    expect(omitted.map((r) => ({ key: r.key, events: r.events }))).toEqual(
      explicit.map((r) => ({ key: r.key, events: r.events })),
    );
  });

  it("a fractional-offset named zone floors on the half hour", () => {
    // Asia/Kolkata is UTC+5:30. 09:17Z is 14:47 local, floored to 14:00 local =
    // 08:30Z. The block key is that UTC instant.
    const rows = rollup([event("2026-01-01T09:17:00.000Z")], {
      by: "block",
      tz: "Asia/Kolkata",
    });
    expect(rows.map((r) => ({ key: r.key, events: r.events }))).toEqual([
      { key: "2026-01-01T08:30:00.000Z", events: 1 },
    ]);
  });
});

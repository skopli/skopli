import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { rollup } from "../src/rollup.ts";
import type { UsageEvent } from "../src/types.ts";

// Timezone day-bucketing conformance, driven by the SHARED fixtures in
// golden/rollup-tz/. The Rust core and every language facade consume the same
// gold, so TS and the native core must agree on every non-UTC day bucket.

type Bucket = { key: string; events: number };
type Case = { name: string; tz: string; timestamps: string[]; expected: Bucket[] };

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

const casesPath = fileURLToPath(new URL("../golden/rollup-tz/cases.json", import.meta.url));
const cases = JSON.parse(readFileSync(casesPath, "utf8")) as Case[];

describe("rollup timezone day bucketing (shared gold)", () => {
  for (const kase of cases) {
    it(kase.name, () => {
      const events = kase.timestamps.map(event);
      const rows = rollup(events, { by: "day", tz: kase.tz });
      expect(rows.map((r) => ({ key: r.key, events: r.events }))).toEqual(kase.expected);
    });
  }

  it("omitted tz matches the resolved system zone", () => {
    // The tz-omitted default resolves the same system zone as passing it
    // explicitly, so both calls must produce identical buckets. Portable: it
    // asserts equality between the two calls, not any particular host zone.
    const systemTz = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const events = [
      event("2026-08-01T23:30:00.000Z"),
      event("2026-08-02T00:30:00.000Z"),
      event("2026-08-02T12:00:00.000Z"),
    ];
    const omitted = rollup(events, { by: "day" });
    const explicit = rollup(events, { by: "day", tz: systemTz });
    expect(omitted.map((r) => ({ key: r.key, events: r.events }))).toEqual(
      explicit.map((r) => ({ key: r.key, events: r.events })),
    );
  });
});

import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { loadCached, storeCached } from "../src/pricing/cache.ts";

// Cache fetchedAt boundary conformance, driven by the SHARED fixtures in
// golden/pricing/timestamps/. The stamp is one wire contract read and written
// by seven facades; TS and the native core must agree on every boundary.

type ReadCase = { name: string; stamp: string; epochMs: number | null };
type Cases = { read: ReadCase[]; write: number[] };

const casesPath = fileURLToPath(
  new URL("../golden/pricing/timestamps/cases.json", import.meta.url),
);
const cases = JSON.parse(readFileSync(casesPath, "utf8")) as Cases;

function writeStamp(dir: string, name: string, stamp: string): void {
  writeFileSync(join(dir, name), JSON.stringify({ fetchedAt: stamp, payload: {} }));
}

describe("cache timestamp boundary read (shared gold)", () => {
  for (const kase of cases.read) {
    it(kase.name, () => {
      const dir = mkdtempSync(join(tmpdir(), "skopli-ts-ts-"));
      const name = "pricing-openrouter.json";
      writeStamp(dir, name, kase.stamp);
      if (kase.epochMs === null) {
        expect(loadCached(dir, name, 0, 0)).toBeNull();
        return;
      }
      // At now == epochMs the age is zero, not stale; one ms later it is stale.
      // The pair pins the parsed epoch to the millisecond.
      expect(loadCached(dir, name, 0, kase.epochMs)?.stale).toBe(false);
      expect(loadCached(dir, name, 0, kase.epochMs + 1)?.stale).toBe(true);
    });
  }
});

describe("cache timestamp boundary write (shared gold)", () => {
  for (const ms of cases.write) {
    it(`writes integral-ms for ${ms}`, () => {
      const dir = mkdtempSync(join(tmpdir(), "skopli-ts-tsw-"));
      const stamp = storeCached(dir, "pricing-openrouter.json", {}, ms);
      expect(stamp).toHaveLength(24);
      expect(stamp.endsWith("Z")).toBe(true);
      expect(Date.parse(stamp)).toBe(ms);
    });
  }
});

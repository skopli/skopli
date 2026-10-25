import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { loadCached, storeCached } from "../src/pricing/cache.ts";
import { dateParseMs } from "../src/time.ts";

type ReadCase = { name: string; stamp: string; epochMs: number | null };
type Cases = { read: ReadCase[]; write: number[] };

const casesPath = fileURLToPath(
  new URL("../golden/pricing/timestamps/cases.json", import.meta.url),
);
const cases = JSON.parse(readFileSync(casesPath, "utf8")) as Cases;

const name = "pricing-openrouter.json";
const cacheWith = (stamp: string): string => {
  const dir = mkdtempSync(join(tmpdir(), "skopli-ts-"));
  writeFileSync(join(dir, name), JSON.stringify({ fetchedAt: stamp, payload: {} }));
  return dir;
};

describe("dateParseMs replays the shared timestamp fixture", () => {
  for (const kase of cases.read) {
    it(kase.name, () => {
      expect(dateParseMs(kase.stamp)).toBe(kase.epochMs ?? undefined);
    });
  }
});

describe("loadCached reads fetchedAt with the shared grammar", () => {
  it("is fresh at the parsed instant and stale one ms later", () => {
    const dir = cacheWith("2026-08-01T00:00:00.000Z");
    expect(loadCached(dir, name, 0, 1785542400000)?.stale).toBe(false);
    expect(loadCached(dir, name, 0, 1785542400001)?.stale).toBe(true);
  });
  for (const stamp of [
    "not-a-timestamp",
    "2026-08-01T00:00:00+0000",
    "Sat, 01 Aug 2026 00:00:00 GMT",
  ]) {
    it(`misses on ${stamp}, whatever the host Date accepts`, () => {
      expect(loadCached(cacheWith(stamp), name, 0, 1785542400000)).toBeNull();
    });
  }
});

describe("storeCached writes the canonical stamp", () => {
  for (const ms of cases.write) {
    it(`writes integral-ms for ${ms}`, () => {
      const stamp = storeCached(mkdtempSync(join(tmpdir(), "skopli-ts-")), name, {}, ms);
      expect(stamp).toHaveLength(24);
      expect(stamp.endsWith("Z")).toBe(true);
      expect(dateParseMs(stamp)).toBe(ms);
    });
  }
});

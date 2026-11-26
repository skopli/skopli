import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { rollupFindings } from "../scripts/check-architecture.mjs";

const facade = (nativeName = "native", wireName = "fromWire") => `
import { native as ${nativeName}, fromWire as ${wireName} } from "./native.ts";
import type { TokenCounts, UsageEvent } from "./types.ts";
export type RollupBy = "model" | "day";
export const DEFAULT_BLOCK_MS = 18_000_000;
export type RollupOptions = { by: RollupBy };
export type Rollup = { key: string; tokens: TokenCounts; events: number; turns: number; calls: number };
export function rollup(events: UsageEvent[], options: RollupOptions): Rollup[] {
  return ${wireName}(${nativeName}().rollup(JSON.stringify(events), JSON.stringify(options)));
}
`;

describe("check-architecture", () => {
  it("accepts the checked-in thin facade", () => {
    expect(rollupFindings(readFileSync("src/rollup.ts", "utf8"))).toEqual([]);
  });

  it("accepts import aliases because they preserve the same delegation", () => {
    expect(rollupFindings(facade("coreNative", "parseWire"))).toEqual([]);
  });

  it("rejects a facade that does not return the native result", () => {
    const source = `${facade().replace("return fromWire(native().rollup", "return [] ?? fromWire(native().rollup")}`;
    expect(rollupFindings(source)).not.toEqual([]);
  });

  it("rejects copied rollup helpers even when their names change", () => {
    const source = `${facade().replace(
      "return fromWire(native().rollup(JSON.stringify(events), JSON.stringify(options)));",
      "const grouped = new Map(); for (const event of events) grouped.set(event.model, event); return [...grouped.keys()].map((key) => ({ key, tokens: events[0].tokens, events: 1, turns: 1, calls: 1 }));",
    )}`;
    expect(rollupFindings(source)).not.toEqual([]);
  });
});

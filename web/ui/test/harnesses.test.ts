import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { harnesses } from "../data/harnesses.ts";
import { harnessIconPaths } from "../data/harness-icons.ts";

const golden = JSON.parse(
  readFileSync(new URL("../../../golden/registry/ids.json", import.meta.url), "utf8"),
) as { ids: string[] };

describe("harness registry", () => {
  it("matches the golden id set exactly", () => {
    expect(harnesses.map((h) => h.id).sort()).toEqual([...golden.ids].sort());
    expect(harnesses).toHaveLength(40);
  });

  it("has unique ids and names", () => {
    expect(new Set(harnesses.map((h) => h.id)).size).toBe(harnesses.length);
    expect(new Set(harnesses.map((h) => h.name)).size).toBe(harnesses.length);
  });

  it("only references icons that exist", () => {
    for (const h of harnesses) {
      if (h.icon) expect(harnessIconPaths[h.icon]).toBeTypeOf("string");
    }
  });
});

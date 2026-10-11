import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { harnesses, unsupported } from "../src/data/harnesses.ts";
import { languages } from "../src/data/languages.ts";

const registry = JSON.parse(
  readFileSync(new URL("../../../golden/registry/ids.json", import.meta.url), "utf8"),
) as {
  ids: string[];
};

test("harness ids equal golden/registry/ids.json", () => {
  expect(harnesses.map((h) => h.id)).toEqual(registry.ids);
  expect(harnesses).toHaveLength(40);
});

test("display names are unique and four tools are detected but unsupported", () => {
  expect(new Set(harnesses.map((h) => h.name)).size).toBe(40);
  expect(unsupported).toHaveLength(4);
});

test("nine language surfaces, each with a quickstart", () => {
  expect(languages).toHaveLength(9);
  for (const l of languages) expect(l.quickstart.trim()).not.toBe("");
});

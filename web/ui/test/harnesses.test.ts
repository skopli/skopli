import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { HARNESSES, HARNESS_COUNT } from "../src/data/harnesses.ts";
import { LANGUAGE_COUNT } from "../src/data/languages.ts";
import { harnessIconPaths } from "../src/data/harness-icons.ts";

const registry = JSON.parse(
  readFileSync(new URL("../../../golden/registry/ids.json", import.meta.url), "utf8"),
) as { ids: string[] };

test("harness ids equal golden/registry/ids.json in order", () => {
  expect(HARNESSES.map((h) => h.id)).toEqual(registry.ids);
  expect(HARNESS_COUNT).toBe(40);
  expect(LANGUAGE_COUNT).toBe(9);
});

test("display names are unique and use the authority spellings", () => {
  const names = HARNESSES.map((h) => h.name);
  expect(new Set(names).size).toBe(names.length);
  for (const name of [
    "GitHub Copilot CLI",
    "Amp CLI",
    "Droid CLI",
    "omp",
    "Gajae-Code",
    "goose",
    "Hermes Agent",
    "Kimi Code CLI",
    "Grok Build",
    "Auggie CLI",
    "Xum",
    "Kilo Code for VS Code",
    "fx",
  ])
    expect(names).toContain(name);
});

test("every icon slug is a registry id", () => {
  for (const slug of Object.keys(harnessIconPaths)) expect(registry.ids).toContain(slug);
});

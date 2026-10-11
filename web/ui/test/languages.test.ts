import { expect, test } from "vitest";
import { LANGUAGES } from "../src/data/languages.ts";

test("nine languages, each with an install snippet", () => {
  expect(LANGUAGES).toHaveLength(9);
  for (const l of LANGUAGES) expect(l.install.length).toBeGreaterThan(0);
});

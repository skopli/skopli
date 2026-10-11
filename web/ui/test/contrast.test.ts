import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { checkContrast } from "../scripts/contrast.ts";

test("every declared text/ground pair meets AA in both themes", () => {
  const css = readFileSync(new URL("../src/styles/tokens.css", import.meta.url), "utf8");
  const checks = checkContrast(css);
  expect(checks.length).toBeGreaterThan(20);
  const failing = checks
    .filter((c) => c.ratio < c.min)
    .map((c) => `${c.theme} ${c.fg}/${c.bg} ${c.ratio.toFixed(2)}`);
  expect(failing).toEqual([]);
});

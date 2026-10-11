import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { parseThemes } from "../src/code-theme.ts";

type Check = { theme: string; fg: string; bg: string; ratio: number; min: number };

const TEXT_ON = ["fg", "fg-muted", "accent", "miss", "ok"];
const SURFACES = ["bg", "bg-raised"];

const luminance = (hex: string): number => {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

export const contrast = (a: string, b: string): number => {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (l1 + 0.05) / (l2 + 0.05);
};

export const checkContrast = (css: string): Check[] => {
  const themes = parseThemes(css);
  const light = themes.light ?? {};
  const out: Check[] = [];
  for (const [theme, overrides] of Object.entries(themes)) {
    const vars = { ...light, ...overrides };
    const pair = (fg: string, bg: string, min: number) =>
      out.push({ theme, fg, bg, ratio: contrast(vars[fg]!, vars[bg]!), min });
    for (const bg of SURFACES) {
      for (const fg of TEXT_ON) pair(fg, bg, 4.5);
      pair("rule", bg, 3);
    }
    pair("on-accent", "accent", 4.5);
    pair("fg", "selection", 4.5);
  }
  return out;
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const css = readFileSync(new URL("../src/styles/tokens.css", import.meta.url), "utf8");
  const checks = checkContrast(css);
  for (const c of checks) {
    const mark = c.ratio >= c.min ? "ok  " : "FAIL";
    console.log(
      `${mark} ${c.theme.padEnd(5)} ${c.fg.padEnd(9)} on ${c.bg.padEnd(9)} ${c.ratio.toFixed(2)} (min ${c.min})`,
    );
  }
  process.exitCode = checks.every((c) => c.ratio >= c.min) ? 0 : 1;
}

// Asserts WCAG AA contrast for the text/ground pairs the UI uses, in both themes.
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/styles/tokens.css", import.meta.url), "utf8");
const blocks = [...css.matchAll(/(:root(?:\[data-theme="(\w+)"\])?)\s*\{([^}]*)\}/g)];
const themes = {};
for (const [, , theme = "dark", body] of blocks) {
  const vars = Object.fromEntries(
    [...body.matchAll(/--([\w-]+):\s*(#[0-9a-f]{6})/gi)].map((m) => [m[1], m[2]]),
  );
  themes[theme] = { ...themes.dark, ...themes[theme], ...vars };
}

const luminance = (hex) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
};
const ratio = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

// [text, ground, minimum]. 4.5 for body text, 3 for large text and UI boundaries.
const pairs = [
  ["fg", "ground", 4.5],
  ["fg", "surface", 4.5],
  ["fg", "sunken", 4.5],
  ["fg-muted", "ground", 4.5],
  ["fg-muted", "surface", 4.5],
  ["fg-muted", "sunken", 4.5],
  ["fg-faint", "ground", 4.5],
  ["fg-faint", "surface", 4.5],
  ["fg-faint", "sunken", 4.5],
  ["accent", "ground", 4.5],
  ["accent", "surface", 4.5],
  ["accent", "sunken", 4.5],
  ["accent-ink", "accent", 4.5],
  ["nodata", "ground", 4.5],
  ["nodata", "surface", 4.5],
  ["nodata", "sunken", 4.5],
  ["fg", "selection", 4.5],
  ["ground", "fg", 4.5],
  ["focus", "ground", 3],
  ["focus", "surface", 3],
  ["focus", "sunken", 3],
  ["rule-strong", "ground", 3],
  ["rule-strong", "surface", 3],
];

let failed = 0;
for (const [theme, vars] of Object.entries(themes)) {
  for (const [text, ground, min] of pairs) {
    const r = ratio(vars[text], vars[ground]);
    const ok = r >= min;
    if (!ok) failed++;
    console.log(
      `${ok ? "ok  " : "FAIL"} ${theme.padEnd(5)} ${text} on ${ground}: ${r.toFixed(2)} (min ${min})`,
    );
  }
}
if (failed) {
  console.error(`${failed} contrast pair(s) below AA`);
  process.exit(1);
}

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

type Tokens = Map<string, string>;

const css = readFileSync(new URL("../styles/tokens.css", import.meta.url), "utf8");

function block(selector: string): Tokens {
  const start = css.indexOf(selector);
  const open = css.indexOf("{", start);
  const close = css.indexOf("}", open);
  const tokens: Tokens = new Map();
  for (const m of css.slice(open, close).matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    tokens.set(m[1]!, m[2]!.replace(/\s+/g, " ").trim());
  }
  return tokens;
}

const dark = block(":root {");
const light = new Map([...dark, ...block(':root[data-theme="light"]')]);

function resolve(tokens: Tokens, name: string): string {
  const value = tokens.get(name);
  if (!value) throw new Error(`missing token ${name}`);
  const ref = /^var\((--[\w-]+)\)$/.exec(value);
  return ref ? resolve(tokens, ref[1]!) : value;
}

function luminance(hex: string): number {
  const channel = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const n = Number.parseInt(hex.slice(1), 16);
  return (
    0.2126 * channel(((n >> 16) & 255) / 255) +
    0.7152 * channel(((n >> 8) & 255) / 255) +
    0.0722 * channel((n & 255) / 255)
  );
}

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

const grounds = ["--ground-0", "--ground-1", "--ground-2", "--ground-3"];
const codeInk = [
  "--code-kw",
  "--code-str",
  "--code-num",
  "--code-fn",
  "--code-cm",
  "--code-pn",
  "--code-txt",
];

export const pairs: { fg: string; bg: string; min: number }[] = [
  ...[
    "--ink-0",
    "--ink-1",
    "--ink-2",
    "--ink-3",
    "--accent",
    "--accent-strong",
    "--ok",
    "--miss",
  ].flatMap((fg) => grounds.map((bg) => ({ fg, bg, min: 4.5 }))),
  ...codeInk.map((fg) => ({ fg, bg: "--code-0", min: 4.5 })),
  { fg: "--code-title-fg", bg: "--code-title-bg", min: 4.5 },
  { fg: "--diff-add", bg: "--diff-add-wash", min: 4.5 },
  { fg: "--diff-del", bg: "--diff-del-wash", min: 4.5 },
  { fg: "--ok", bg: "--ok-wash", min: 4.5 },
  { fg: "--miss", bg: "--miss-wash", min: 4.5 },
  { fg: "--accent", bg: "--accent-wash", min: 4.5 },
  { fg: "--ink-inv", bg: "--accent", min: 4.5 },
  ...["--rule-ui", "--accent"].flatMap((fg) => grounds.map((bg) => ({ fg, bg, min: 3 }))),
];

export function audit(): string[] {
  const failures: string[] = [];
  for (const [mode, tokens] of [
    ["dark", dark],
    ["light", light],
  ] as const) {
    for (const { fg, bg, min } of pairs) {
      const ratio = contrast(resolve(tokens, fg), resolve(tokens, bg));
      if (ratio < min) failures.push(`${mode} ${fg} on ${bg}: ${ratio.toFixed(2)} < ${min}`);
    }
  }
  return failures;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const failures = audit();
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    process.exit(1);
  }
  console.log(`contrast: ${pairs.length * 2} pairs pass`);
}

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const abbreviation = /\b(e\.g|i\.e|etc|vs|cf|approx|No|fig|vol|ch|Dr|Mr|Mrs|Ms|St|Inc|Ltd)\.$/;
const marker = /^(\s*>)*\s*(([-*+]|\d+\.)\s+)?/;
const notProse = /^(#|\||<|\[[^\]]+\]:|$)/;
const spans = /`[^`]*`|\([^)]*\)|\[[^\]]*\]/g;
const boundary = /[.!?]["')]*\s+(?=[A-Z`*])/g;

// Blanks every HTML comment in the whole document, keeping its newlines so line
// numbers survive. An unterminated comment runs to the end of the document.
export const stripComments = (text) => {
  let out = "";
  let from = 0;
  for (;;) {
    const open = text.indexOf("<!--", from);
    if (open < 0) return out + text.slice(from);
    const close = text.indexOf("-->", open + 4);
    const end = close < 0 ? text.length : close + 3;
    out += text.slice(from, open) + text.slice(open, end).replace(/[^\n]/g, "");
    from = end;
  }
};

export const paragraphLines = (text) => {
  const out = [];
  let fenced = false;
  let front = text.startsWith("---\n");
  stripComments(text)
    .split("\n")
    .forEach((raw, i) => {
      if (front) {
        front = !(i > 0 && raw === "---");
        return;
      }
      const line = raw.replace(marker, "");
      if (/^(```|~~~)/.test(line)) {
        fenced = !fenced;
        return;
      }
      if (!fenced && !notProse.test(line)) out.push([line, i + 1]);
    });
  return out;
};

export const twoSentences = (line) => {
  const masked = line.replace(spans, (m) => m[0] + "x".repeat(m.length - 2) + m.at(-1));
  for (const m of masked.matchAll(boundary)) {
    if (!abbreviation.test(masked.slice(0, m.index + 1))) return true;
  }
  return false;
};

export const rules = [
  {
    name: "em dash",
    fix: "write the sentence with a plain dash, a comma, a colon or a full stop",
    lines: (text) =>
      text.split("\n").flatMap((line, i) => (line.includes("\u2014") ? [i + 1] : [])),
  },
  {
    name: "two sentences on one line",
    fix: "put each sentence of a Markdown paragraph on its own line (Markdown joins them)",
    lines: (text, file) =>
      /\.mdx?$/.test(file)
        ? paragraphLines(text).flatMap(([line, n]) => (twoSentences(line) ? [n] : []))
        : [],
  },
];

const main = () => {
  const args = process.argv.slice(2);
  if (args.length) {
    const help = args.includes("--help") || args.includes("-h");
    (help ? console.log : console.error)(
      [
        "usage: node scripts/check-prose.mjs",
        "",
        "Reads every tracked file (third-party text under golden/<harness>/<case>/input/ and",
        "THIRD-PARTY-NOTICES.md excepted) and fails on an em dash anywhere or on two sentences in",
        "one Markdown line. Runs inside pnpm run lint; no options, no files to pass.",
        "",
        "Not detected: a second sentence opening with a lowercase word or a digit. Not skipped:",
        "a code block indented four spaces instead of fenced; fence it. HTML comments are",
        "blanked over the whole file before the line checks, fenced code included, so an",
        "unterminated <!-- hides everything after it.",
        "",
        "  exit 0  every rule holds",
        "  exit 1  findings, one file:line: rule per line on stderr, then the fix per rule",
        "  exit 2  unknown argument, this usage on stderr",
      ].join("\n"),
    );
    process.exit(help ? 0 : 2);
  }
  process.chdir(join(import.meta.dirname, ".."));

  const thirdParty = (file) =>
    file === "THIRD-PARTY-NOTICES.md" || /^golden\/[^/]+\/[^/]+\/input\//.test(file);
  const files = execFileSync("git", ["ls-files", "-z"], { encoding: "utf8" })
    .split("\0")
    .filter((file) => file && !thirdParty(file));

  const findings = [];
  for (const file of files) {
    const bytes = readFileSync(file);
    if (bytes.includes(0)) continue;
    const text = bytes.toString("utf8");
    for (const rule of rules) {
      for (const line of rule.lines(text, file)) findings.push(`${file}:${line}: ${rule.name}`);
    }
  }

  if (findings.length) {
    console.error(findings.join("\n"));
    for (const rule of rules) {
      if (findings.some((f) => f.endsWith(`: ${rule.name}`)))
        console.error(`${rule.name}: ${rule.fix}`);
    }
    process.exit(1);
  }
  console.log(`prose rules hold on ${files.length} tracked files`);
};

if (process.argv[1] === import.meta.filename) main();

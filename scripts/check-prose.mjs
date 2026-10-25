import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

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
      "  exit 0  every rule holds",
      "  exit 1  findings, one file:line: rule per line on stderr, then the fix per rule",
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

const abbreviation = /\b(e\.g|i\.e|etc|vs|cf|approx|no|fig|vol|ch|Dr|Mr|Mrs|Ms|St|Inc|Ltd)\.$/i;
const marker = /^(\s*>)*\s*(([-*+]|\d+\.)\s+)?/;
const notProse = /^(#|\||<|\[[^\]]+\]:|$)/;
const spans = /`[^`]*`|\([^)]*\)|\[[^\]]*\]/g;
const boundary = /[.!?]["')]*\s+(?=[A-Z`*])/g;

const paragraphLines = (text) => {
  const out = [];
  let fenced = false;
  let comment = false;
  let front = text.startsWith("---\n");
  text.split("\n").forEach((raw, i) => {
    if (front) {
      front = !(i > 0 && raw === "---");
      return;
    }
    if (/^\s*(```|~~~)/.test(raw)) {
      fenced = !fenced;
      return;
    }
    if (fenced) return;
    let line = raw;
    if (comment) {
      const end = line.indexOf("-->");
      if (end < 0) return;
      comment = false;
      line = line.slice(end + 3);
    }
    line = line.replace(/<!--.*?-->/g, "");
    const open = line.indexOf("<!--");
    if (open >= 0) {
      comment = true;
      line = line.slice(0, open);
    }
    const body = line.replace(marker, "");
    if (!notProse.test(body)) out.push([body, i + 1]);
  });
  return out;
};

const twoSentences = (line) => {
  const masked = line.replace(spans, (m) => m[0] + "x".repeat(m.length - 2) + m.at(-1));
  for (const m of masked.matchAll(boundary)) {
    if (!abbreviation.test(masked.slice(0, m.index + 1))) return true;
  }
  return false;
};

const rules = [
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

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
      "Reads every tracked file (third-party text under golden/<harness>/<case>/input/ excepted)",
      "and fails on an em dash. Runs inside pnpm run lint; no options, no files to pass.",
      "",
      "  exit 0  every rule holds",
      "  exit 1  findings, one file:line: rule per line on stderr, then the fix per rule",
    ].join("\n"),
  );
  process.exit(help ? 0 : 2);
}
process.chdir(join(import.meta.dirname, ".."));

const thirdParty = (file) => /^golden\/[^/]+\/[^/]+\/input\//.test(file);
const files = execFileSync("git", ["ls-files", "-z"], { encoding: "utf8" })
  .split("\0")
  .filter((file) => file && !thirdParty(file));

const rules = [
  {
    name: "em dash",
    fix: "write the sentence with a plain dash, a comma, a colon or a full stop",
    lines: (text) =>
      text.split("\n").flatMap((line, i) => (line.includes("\u2014") ? [i + 1] : [])),
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

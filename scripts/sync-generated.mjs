// Regenerates files derived from a single source; `--check` fails when a committed copy differs.
//   README.md install table                     <- web/ui/src/quickstart.ts
//   llms.txt supported-harness list             <- golden/registry/ids.json
//   web/docs/src/content/docs/contributing.md   <- CONTRIBUTING.md
import { readFileSync, writeFileSync } from "node:fs";
import { harnesses } from "../web/ui/src/harnesses.ts";
import { langs } from "../web/ui/src/langs.ts";
import { installLine, published, quickstarts, releaseNote } from "../web/ui/src/quickstart.ts";

const check = process.argv.includes("--check");
const repo = "https://github.com/skopli/skopli";
const ids = JSON.parse(readFileSync("golden/registry/ids.json", "utf8")).ids;

const replaceBetween = (text, start, end, body) => {
  const re = new RegExp(`(${start})\\n[\\s\\S]*?(${end})`);
  if (!re.test(text)) throw new Error(`markers ${start} / ${end} not found`);
  return text.replace(re, `$1\n\n${body}\n\n$2`);
};

const installTable = () => {
  const header = published
    ? ["Language", "Registry", "Install"]
    : ["Language", "Install now (from the repository)", "After the first release"];
  const rows = quickstarts.map((q) => {
    const { command, note } = installLine(q);
    const now = `\`${command}\`${note ? ` (${note})` : ""}`;
    const later =
      q.install === q.source ? "same (git dependency)" : `${q.registry}: \`${q.install}\``;
    return published ? [langs[q.id], q.registry, now] : [langs[q.id], now, later];
  });
  const widths = header.map((_, i) => Math.max(...[header, ...rows].map((r) => r[i].length)));
  const line = (cells) => `| ${cells.map((c, i) => c.padEnd(widths[i])).join(" | ")} |`;
  return [
    releaseNote,
    "",
    line(header),
    line(widths.map((w) => "-".repeat(w))),
    ...rows.map(line),
  ].join("\n");
};

const harnessLine = (text) => {
  const re = /(## Supported harnesses\n\n)([^.\n]+)\.([^\n]*)/;
  const m = text.match(re);
  if (!m) throw new Error("llms.txt: supported-harness paragraph not found");
  const named = harnesses.filter((h) => m[3].includes(h.name)).map((h) => h.name);
  if (named.length)
    throw new Error(`llms.txt lists supported harnesses as unsupported: ${named.join(", ")}`);
  return text.replace(re, `$1${ids.join(", ")}.$3`);
};

const contributingPage = (md) => {
  const body = md
    .replace(/^# .*\n\n/, "")
    .replace(/\]\((?!https?:|#)([^)]+)\)/g, `](${repo}/blob/main/$1)`);
  return `---\ntitle: Contributing\ndescription: How to build and test Skopli, add a harness or a pricing source, and what a pull request needs.\n---\n\n<!-- generated from CONTRIBUTING.md by scripts/sync-generated.mjs; edit the source -->\n\n${body}`;
};

const targets = [
  [
    "README.md",
    (t) =>
      replaceBetween(t, "<!-- generated: install-table -->", "<!-- /generated -->", installTable()),
  ],
  ["llms.txt", harnessLine],
  [
    "web/docs/src/content/docs/contributing.md",
    () => contributingPage(readFileSync("CONTRIBUTING.md", "utf8")),
  ],
];

let stale = [];
for (const [file, render] of targets) {
  const current = (() => {
    try {
      return readFileSync(file, "utf8");
    } catch {
      return "";
    }
  })();
  const next = render(current);
  if (next === current) continue;
  if (check) stale.push(file);
  else writeFileSync(file, next);
}
if (stale.length) {
  console.error(`out of date: ${stale.join(", ")}\nregenerate with: pnpm run sync`);
  process.exit(1);
}
console.log(check ? "generated files are up to date" : "generated files written");

import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { harnesses } from "../web/ui/src/harnesses.ts";
import { langs } from "../web/ui/src/langs.ts";
import { command, published, quickstarts, releaseNote } from "../web/ui/src/quickstart.ts";

process.chdir(join(import.meta.dirname, ".."));
const check = process.argv.includes("--check");
const repo = "https://github.com/skopli/skopli";
const read = (file) => readFileSync(file, "utf8");
const ids = JSON.parse(read("golden/registry/ids.json")).ids;
if (harnesses.map((h) => h.id).join() !== ids.join()) {
  throw new Error("web/ui/src/harnesses.ts does not match golden/registry/ids.json");
}

const replaceBetween = (text, start, end, body) => {
  const from = text.indexOf(start);
  const to = text.indexOf(end, from);
  if (from < 0 || to < 0) throw new Error(`markers ${start} / ${end} not found`);
  return `${text.slice(0, from + start.length)}\n\n${body}\n\n${text.slice(to)}`;
};

const installTable = () => {
  const header = published
    ? ["Language", "Registry", "Install", "Notes"]
    : ["Language", "Install now (from the repository)", "Notes", "After the first release"];
  const rows = quickstarts.map((q) => {
    const cmd = `\`${command(q)}\``;
    const note = q.note ?? "";
    return published
      ? [langs[q.id], q.registry ?? "none", cmd, note]
      : [
          langs[q.id],
          cmd,
          note,
          q.registry ? `${q.registry}: \`${q.install}\`` : "same command, no registry",
        ];
  });
  const widths = header.map((_, i) => Math.max(...[header, ...rows].map((r) => r[i].length)));
  const line = (cells) => `| ${cells.map((c, i) => c.padEnd(widths[i])).join(" | ")} |`;
  return [
    ...(releaseNote ? [releaseNote, ""] : []),
    line(header),
    line(widths.map((w) => "-".repeat(w))),
    ...rows.map(line),
  ].join("\n");
};

const harnessLine = (text) => {
  const re = /(## Supported harnesses\n\n)([^.\n]+)\.([^\n]*)/;
  const m = text.match(re);
  if (!m) throw new Error("llms.txt: supported-harness paragraph not found");
  const words = new Set(m[3].split(/[,.:;()]/).map((w) => w.trim().toLowerCase()));
  const named = harnesses
    .filter((h) => [h.id, h.name, h.name.split(" ")[0]].some((n) => words.has(n.toLowerCase())))
    .map((h) => h.name);
  if (named.length) {
    throw new Error(`llms.txt lists supported harnesses as unsupported: ${named.join(", ")}`);
  }
  return text.replace(re, `$1${ids.join(", ")}.$3`);
};

const contributingPage = () => {
  const body = read("CONTRIBUTING.md")
    .replace(/^# .*\n\n/, "")
    .replace(/`([^`\s<>*]+)`/g, (m, path) =>
      existsSync(path)
        ? `[${m}](${repo}/${statSync(path).isDirectory() ? "tree" : "blob"}/main/${path})`
        : m,
    );
  return `---\ntitle: Contributing\ndescription: How to build and test Skopli, add a harness or a pricing source, and what a pull request needs.\n---\n\n${body}`;
};

const targets = [
  [
    "README.md",
    (t) =>
      replaceBetween(t, "<!-- generated: install-table -->", "<!-- /generated -->", installTable()),
  ],
  ["llms.txt", harnessLine],
  ["web/docs/src/content/docs/contributing.md", contributingPage],
];

const stale = [];
for (const [file, render] of targets) {
  const current = read(file);
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

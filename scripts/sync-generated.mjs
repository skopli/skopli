import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { harnesses, label } from "../web/ui/src/harnesses.ts";
import { langs } from "../web/ui/src/langs.ts";
import { command, published, quickstarts, releaseNote } from "../web/ui/src/quickstart.ts";

const args = process.argv.slice(2);
if (args.some((a) => a !== "--check")) {
  const help = args.includes("--help") || args.includes("-h");
  (help ? console.log : console.error)(
    [
      "usage: pnpm run sync [--check]",
      "",
      "Regenerates every committed derived file: the README install and harness tables, the",
      "llms.txt harness list, the docs copy of CONTRIBUTING.md, the C header and the C# bindings.",
      "",
      "  --check  leave every file as it is; exit 1 and list the stale files instead",
    ].join("\n"),
  );
  process.exit(help ? 0 : 2);
}
process.chdir(join(import.meta.dirname, ".."));
const check = args.includes("--check");
const repo = "https://github.com/skopli/skopli";
const read = (file) => readFileSync(file, "utf8");
const ids = JSON.parse(read("golden/registry/ids.json")).ids;
if (harnesses.map((h) => h.id).join() !== ids.join()) {
  throw new Error("web/ui/src/harnesses.ts does not match golden/registry/ids.json");
}
const icons = new Set(harnesses.map((h) => h.icon));
const orphanLogos = readdirSync("assets/logos").filter(
  (f) => f.endsWith(".svg") && !icons.has(f.slice(0, -4)),
);
if (orphanLogos.length) {
  throw new Error(
    `assets/logos/{${orphanLogos.join(",")}}: no harness in web/ui/src/harnesses.ts has this icon id; name the file after the icon id or remove it`,
  );
}

const replaceBetween = (text, name, body) => {
  const start = `<!-- generated: ${name} -->`;
  const end = "<!-- /generated -->";
  const from = text.indexOf(start);
  const to = text.indexOf(end, from);
  if (from < 0 || to < 0) throw new Error(`markers ${start} / ${end} not found`);
  return `${text.slice(0, from + start.length)}\n\n${body}\n\n${text.slice(to)}`;
};

const table = (header, rows) => {
  const widths = header.map((_, i) => Math.max(3, ...[header, ...rows].map((r) => r[i].length)));
  const line = (cells) => `| ${cells.map((c, i) => c.padEnd(widths[i])).join(" | ")} |`;
  return [line(header), line(widths.map((w) => "-".repeat(w))), ...rows.map(line)].join("\n");
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
  return [...(releaseNote ? [releaseNote, ""] : []), table(header, rows)].join("\n");
};

const harnessTable = () => {
  const cell = (h) => {
    const logo = `assets/logos/${h.icon}.svg`;
    const img =
      h.icon && existsSync(logo)
        ? `<img src="${logo}" height="16" alt="" align="absmiddle" /> `
        : "";
    return [`${img}${label(h)}`, `\`${h.id}\``];
  };
  const half = Math.ceil(harnesses.length / 2);
  const rows = harnesses.slice(0, half).map((h, i) => {
    const right = harnesses[half + i];
    return [...cell(h), "", ...(right ? cell(right) : ["", ""])];
  });
  return [
    `${harnesses.length} supported.`,
    "The id is what you pass in `readUsage({ harnesses: [...] })`.",
    "",
    table(["Harness", "id", "", "Harness", "id"], rows),
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

const readme = (t) => {
  const next = replaceBetween(
    replaceBetween(t, "install-table", installTable()),
    "harness-table",
    harnessTable(),
  );
  const missing = [...next.matchAll(/src="(assets\/logos\/[^"]+)"/g)]
    .map((m) => m[1])
    .filter((logo) => !existsSync(logo));
  if (missing.length) {
    throw new Error(`README.md points at missing logos: ${missing.join(", ")}`);
  }
  return next;
};

const targets = [
  ["README.md", readme],
  ["llms.txt", harnessLine],
  ["web/docs/src/content/docs/contributing.md", contributingPage],
];

const stale = [];
const cargoGenerated = [
  "crates/skopli-capi/include/skopli.h",
  "sdks/csharp/Skopli/Native/NativeMethods.g.cs",
];
const snapshot = (file) => (existsSync(file) ? read(file) : null);
const before = cargoGenerated.map(snapshot);
const cargo = spawnSync("cargo", ["build", "--quiet", "-p", "skopli-capi"], {
  stdio: "inherit",
  env: { ...process.env, SKOPLI_GEN_HEADER: "1", SKOPLI_GEN_CSHARP: "1" },
});
cargoGenerated.forEach((file, i) => {
  if (snapshot(file) === before[i]) return;
  if (check) {
    stale.push(file);
    if (before[i] === null) rmSync(file);
    else writeFileSync(file, before[i]);
  }
});
if (cargo.status !== 0) {
  console.error(
    cargo.error?.code === "ENOENT"
      ? "cargo not found on PATH; it regenerates the C header and the C# bindings"
      : "cargo build -p skopli-capi failed",
  );
  process.exit(1);
}

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

#!/usr/bin/env node
// Promote a redacted harness capture into golden/<harness>/<case>/ and generate its expected files.
// Redaction rules apply to text files, to zstd-compressed text, and to every text column of
// SQLite databases (WAL checkpointed first, then VACUUMed). Each rule also applies in its
// dash-joined (`-home-user`) and percent-encoded (`%2Fhome%2Fuser`) forms so project-slug
// directory names stay consistent with the paths inside the files; longer sources apply first
// so a short rule (`ubuntu=user`) cannot consume the path a longer rule names.
// After copying, every written file is checked: no redaction source may survive in its bytes
// (zstd files are checked decompressed), git must not ignore it, and a non-text file must carry
// an explicit binary attribute so checkin never applies eol conversion to it. Any hit exits 1.
// The manifest points `home` at input/ (or the named env var at <rel>), expected-events.json
// and expected-rollup.json come from the repository's own readers through replay.mjs, and
// README.md records provenance. Prints the resulting totals as JSON on stdout.
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { zstdCompressSync, zstdDecompressSync } from "node:zlib";
import { parseArgs } from "./cli.mjs";
import { replayCase, root } from "./replay.mjs";

const help = `usage: promote.mjs <capture-home> <harness>/<case> --store <rel>... [options]

Copies the named stores of a harness capture into golden/<harness>/<case>/input/
with the redaction rules applied to file names and contents, regenerates the
expected files through replay.mjs and writes the case README. The case directory
is removed and rewritten on every run.

  --store <rel>         path under <capture-home> to copy (repeatable, required)
  --redact <from>=<to>  text to replace; also applied dash-joined and percent-
                        encoded, longest source first (repeatable)
  --env NAME=<rel>      point the manifest env var at input/<rel> instead of home
  --gateway <text>      README: gateway description
  --server <text>       README: server description
  --model <text>        README: model description
  --cache <text>        README: cache source (default server-reported)
  --note <text>         README: extra bullet (repeatable)
  --dry-run             print the resolved rules and the files that would be
                        written as JSON and exit without touching golden/
  --quiet               no progress lines on stderr
  --help                this text

exit codes: 0 ok, 1 a written file failed the redaction, ignore or attribute
check (the case is removed again), 2 usage error

examples:
  promote.mjs /tmp/cap/home omp/openai-chat --store .omp/agent/sessions \\
    --redact /tmp/cap/home=/home/user --redact "$(hostname)=host" --cache server-reported
  promote.mjs /tmp/cap/home jcode/anthropic-messages --store .jcode/sessions --dry-run`;
const { positional, many, one, dryRun, log, usage } = parseArgs(help, [
  "--store",
  "--redact",
  "--env",
  "--gateway",
  "--server",
  "--model",
  "--cache",
  "--note",
]);
const [captureHome, caseId] = positional;
const stores = many("--store");
if (!captureHome || !caseId?.includes("/") || stores.length === 0) {
  usage("promote: <capture-home>, <harness>/<case> and at least one --store are required");
}
const [harness, name] = caseId.split("/");
const dir = join(root, "golden", harness, name);
const input = join(dir, "input");

const pair = (s) => {
  const i = s.indexOf("=");
  return [s.slice(0, i), s.slice(i + 1)];
};
const dashed = (p) => p.replace(/^\/+/, "").replaceAll("/", "-");
const rules = [
  ...new Map(
    many("--redact")
      .map(pair)
      .flatMap(([from, to]) => [
        [from, to],
        [dashed(from), dashed(to)],
        [encodeURIComponent(from), encodeURIComponent(to)],
      ])
      .map((rule) => [rule[0], rule]),
  ).values(),
].sort((a, b) => b[0].length - a[0].length);
const redact = (s) => rules.reduce((acc, [from, to]) => acc.replaceAll(from, to), s);
// Same first-8000-bytes NUL heuristic git uses to tell text from binary.
const isText = (buf) => !buf.subarray(0, 8000).includes(0);
const isZstd = (path) => /\.zstd?$/.test(path);
const isSqlite = (buf) => buf.subarray(0, 15).toString("latin1") === "SQLite format 3";

const redactSqlite = (src, dst) => {
  const scratch = join(tmpdir(), `promote-${process.pid}`);
  mkdirSync(scratch, { recursive: true });
  const db = join(scratch, "db");
  copyFileSync(src, db);
  for (const suffix of ["-wal", "-shm"]) {
    try {
      copyFileSync(src + suffix, db + suffix);
    } catch {}
  }
  const conn = new DatabaseSync(db);
  conn.exec("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;");
  // Harness triggers guard row transitions, not redaction: suspend them while rewriting text.
  const triggers = conn.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'trigger'").all();
  for (const { name } of triggers) conn.exec(`DROP TRIGGER "${name}"`);
  // Virtual tables and their shadow tables are not updatable; FTS5 indexes are rebuilt from
  // the redacted content tables afterwards.
  const all = conn.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table'").all();
  const virtual = all.filter(({ sql }) => /^CREATE VIRTUAL TABLE/i.test(sql ?? ""));
  const shadow = ({ name }) =>
    virtual.some((v) => name === v.name || name.startsWith(`${v.name}_`));
  for (const { name: table } of all.filter((t) => !shadow(t))) {
    const cols = conn.prepare(`PRAGMA table_info("${table}")`).all();
    for (const { name: col } of cols) {
      const expr = rules.reduce(
        (acc, [from, to]) => `replace(${acc}, ${quote(from)}, ${quote(to)})`,
        `"${col}"`,
      );
      conn.exec(`UPDATE "${table}" SET "${col}" = ${expr} WHERE typeof("${col}") = 'text'`);
    }
  }
  for (const { name, sql } of virtual) {
    if (/USING fts5/i.test(sql)) conn.exec(`INSERT INTO "${name}"("${name}") VALUES('rebuild')`);
  }
  for (const { sql } of triggers) conn.exec(sql);
  conn.exec("VACUUM");
  conn.close();
  copyFileSync(db, dst);
  rmSync(scratch, { recursive: true, force: true });
};
const quote = (s) => `'${s.replaceAll("'", "''")}'`;

const zstdFrames = (buf) => {
  const parts = [];
  for (let rest = buf; rest.length;) {
    const { buffer, engine } = zstdDecompressSync(rest, { info: true });
    parts.push(buffer);
    rest = rest.subarray(engine.bytesWritten);
  }
  return Buffer.concat(parts);
};

const planned = [];
const copyTree = (from, to) => {
  const stat = statSync(from);
  if (stat.isDirectory()) {
    if (!dryRun) mkdirSync(to, { recursive: true });
    for (const entry of readdirSync(from)) copyTree(join(from, entry), join(to, redact(entry)));
    return;
  }
  if (/\.db-(wal|shm)$|\.sqlite3?-(wal|shm)$/.test(from)) return;
  planned.push(relative(root, to));
  if (dryRun) return;
  const buf = readFileSync(from);
  mkdirSync(join(to, ".."), { recursive: true });
  if (isSqlite(buf)) return redactSqlite(from, to);
  if (isZstd(from)) {
    return writeFileSync(to, zstdCompressSync(redact(zstdFrames(buf).toString("utf8"))));
  }
  if (isText(buf)) return writeFileSync(to, redact(buf.toString("utf8")));
  writeFileSync(to, buf);
};

const files = (p) =>
  statSync(p).isDirectory() ? readdirSync(p).flatMap((e) => files(join(p, e))) : [p];
const git = (args, input) => {
  const run = spawnSync("git", args, { cwd: root, input, encoding: "utf8" });
  if (run.status === null || run.status > 1) throw new Error(`git ${args[0]}: ${run.stderr}`);
  return run.stdout;
};
const fail = (what, items) => {
  if (items.length === 0) return;
  console.error(`promote: ${what}:\n  ${items.join("\n  ")}`);
  rmSync(dir, { recursive: true, force: true });
  process.exit(1);
};
const verifyWritten = () => {
  const written = files(input).map((p) => [relative(root, p), readFileSync(p)]);
  fail(
    "redaction source still present (extend --redact or drop the store)",
    written.flatMap(([p, buf]) => {
      const bytes = isZstd(p) ? zstdFrames(buf) : buf;
      return rules.filter(([from]) => bytes.includes(from)).map(([from]) => `${p}: ${from}`);
    }),
  );
  const paths = written.map(([p]) => p);
  fail(
    "written files are ignored by .gitignore (narrow the pattern)",
    git(["check-ignore", "--no-index", "--stdin"], `${paths.join("\n")}\n`)
      .split("\n")
      .filter(Boolean),
  );
  const binary = written.filter(([, buf]) => !isText(buf)).map(([p]) => p);
  const attr = binary.length
    ? git(["check-attr", "text", "-z", "--stdin"], `${binary.join("\0")}\0`).split("\0")
    : [];
  fail(
    "binary files without a `binary` attribute (add their pattern to .gitattributes)",
    binary.filter((_, i) => attr[i * 3 + 2] !== "unset"),
  );
};

if (!dryRun) rmSync(dir, { recursive: true, force: true });
for (const rel of stores) {
  log(`promote: ${rel} -> ${relative(root, join(input, redact(rel)))}`);
  copyTree(join(captureHome, rel), join(input, redact(rel)));
}
if (dryRun) {
  console.log(
    JSON.stringify({ case: caseId, dir: relative(root, dir), rules, files: planned }, null, 2),
  );
  process.exit(0);
}
verifyWritten();

const env = many("--env").map(pair);
const manifest = {
  schema_version: 1,
  harness,
  case: name,
  ...(env.length ? { env: Object.fromEntries(env) } : { home: "." }),
  options: { harnesses: [harness], tz: "UTC" },
  rollups: ["model", "day", "harness"],
};
writeFileSync(join(dir, "input-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

const { events, by, result, relativize } = await replayCase(caseId);
// Same normalisation the Rust conformance replay applies: paths relative to input/, sorted,
// one warning-severity diagnostic per reader warning, skipped grouped by harness only when non-empty.
const diagnostics = result.diagnostics
  .map((d) => relativize(d.message.replace(/\n$/, "")))
  .sort()
  .map((message) => ({ severity: "warning", message, harness }));
const skippedPaths = Object.values(result.skipped).flat().map(relativize).sort();
const skipped = skippedPaths.length ? { [harness]: skippedPaths } : {};
writeFileSync(
  join(dir, "expected-events.json"),
  `${JSON.stringify({ schema_version: 1, events, diagnostics, skipped }, null, 2)}\n`,
);
writeFileSync(
  join(dir, "expected-rollup.json"),
  `${JSON.stringify({ schema_version: 1, by }, null, 2)}\n`,
);

const totals = by.harness[0]?.tokens ?? {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  reasoning: 0,
};
const readme = [
  `# ${harness} / ${name}`,
  "",
  "Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.",
  "",
  `- Wire shape: ${name}.`,
  `- Gateway: ${one("--gateway", "usage-rewriting gateway in front of a local llama-server")}.`,
  `- Server: ${one("--server", "llama-server")}.`,
  `- Model: ${one("--model", "local model")}.`,
  `- Cache numbers: ${one("--cache", "server-reported")}.`,
  `- Store paths: ${stores.map((s) => `\`${redact(s)}\``).join(", ")}.`,
  `- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.`,
  `- Events ${events.length}, sessions ${new Set(events.map((e) => e.sessionId)).size}, diagnostics ${result.diagnostics.length}, skipped ${Object.values(result.skipped).flat().length}.`,
  `- Totals: input ${totals.input}, output ${totals.output}, cache read ${totals.cacheRead}, cache write ${totals.cacheWrite}, reasoning ${totals.reasoning}.`,
  ...many("--note").map((n) => `- ${n}`),
  "",
];
writeFileSync(join(dir, "README.md"), readme.join("\n"));
log(`promote: wrote ${relative(root, dir)} (${planned.length} input files)`);
console.log(
  JSON.stringify({
    case: caseId,
    events: events.length,
    sessions: new Set(events.map((e) => e.sessionId)).size,
    diagnostics: result.diagnostics.length,
    skipped: Object.values(result.skipped).flat().length,
    totals,
    models: by.model.map((m) => m.key),
  }),
);

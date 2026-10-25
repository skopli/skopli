#!/usr/bin/env node
// Promote a redacted harness capture into golden/<harness>/<case>/ and generate its expected files.
//
//   promote.mjs <capture-home> <harness>/<case> --store <rel>... [--redact <from>=<to>]...
//               [--env NAME=<rel>] [--gateway <text>] [--server <text>] [--model <text>]
//               [--cache server-reported|gateway-derived] [--note <text>]...
//
// <capture-home> is the harness home directory the capture wrote into (kept outside the repo).
// Every --store path is copied under input/ with path components and file contents redacted.
// Redaction rules apply to text files, to zstd-compressed text, and to every text column of
// SQLite databases (WAL checkpointed first, then VACUUMed). Each rule also applies in its
// dash-joined (`-home-user`) and percent-encoded (`%2Fhome%2Fuser`) forms so project-slug
// directory names stay consistent with the paths inside the files.
// The manifest points `home` at input/ (or the named env var at <rel>), expected-events.json
// and expected-rollup.json come from the repository's own readers through replay.mjs, and
// README.md records provenance. Prints the resulting totals as JSON on stdout.
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
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { zstdCompressSync, zstdDecompressSync } from "node:zlib";
import { replayCase, root } from "./replay.mjs";

const argv = process.argv.slice(2);
const [captureHome, caseId] = argv;
const many = (flag) => argv.flatMap((a, i) => (a === flag && argv[i + 1] ? [argv[i + 1]] : []));
const one = (flag, fallback) => many(flag)[0] ?? fallback;
const stores = many("--store");
if (!captureHome || !caseId?.includes("/") || stores.length === 0) {
  console.error("usage: promote.mjs <capture-home> <harness>/<case> --store <rel>... [options]");
  process.exit(2);
}
const [harness, name] = caseId.split("/");
const dir = join(root, "golden", harness, name);
const input = join(dir, "input");

const pair = (s) => {
  const i = s.indexOf("=");
  return [s.slice(0, i), s.slice(i + 1)];
};
const dashed = (p) => p.replace(/^\/+/, "").replaceAll("/", "-");
const rules = many("--redact")
  .map(pair)
  .flatMap(([from, to]) => [
    [from, to],
    [dashed(from), dashed(to)],
    [encodeURIComponent(from), encodeURIComponent(to)],
  ]);
const redact = (s) => rules.reduce((acc, [from, to]) => acc.replaceAll(from, to), s);
const isText = (buf) => !buf.subarray(0, 8192).includes(0);
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

// Append-only logs hold one zstd frame per flush; the one-shot API stops after the first frame.
const zstdFrames = (buf) => {
  const parts = [];
  for (let rest = buf; rest.length;) {
    const { buffer, engine } = zstdDecompressSync(rest, { info: true });
    parts.push(buffer);
    rest = rest.subarray(engine.bytesWritten);
  }
  return Buffer.concat(parts).toString("utf8");
};

const copyTree = (from, to) => {
  const stat = statSync(from);
  if (stat.isDirectory()) {
    mkdirSync(to, { recursive: true });
    for (const entry of readdirSync(from)) copyTree(join(from, entry), join(to, redact(entry)));
    return;
  }
  if (/\.db-(wal|shm)$|\.sqlite3?-(wal|shm)$/.test(from)) return;
  const buf = readFileSync(from);
  mkdirSync(join(to, ".."), { recursive: true });
  if (isSqlite(buf)) return redactSqlite(from, to);
  if (from.endsWith(".zstd") || from.endsWith(".zst")) {
    return writeFileSync(to, zstdCompressSync(Buffer.from(redact(zstdFrames(buf)))));
  }
  if (isText(buf)) return writeFileSync(to, redact(buf.toString("utf8")));
  writeFileSync(to, buf);
};

rmSync(dir, { recursive: true, force: true });
for (const rel of stores) copyTree(join(captureHome, rel), join(input, redact(rel)));

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

const { events, by, result } = await replayCase(caseId);
// Same normalisation the Rust conformance replay applies: paths relative to input/, sorted,
// one warning-severity diagnostic per reader warning, skipped grouped by harness only when non-empty.
const relativize = (s) => s.replaceAll(input, "").replace(/(^|\s)\/+/g, "$1");
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

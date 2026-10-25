#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { parseArgs } from "./cli.mjs";
import { canonical, replayCase, root } from "./replay.mjs";

const help = `usage: golden.mjs <harness>/<case> --out <dir> [--dry-run] [--quiet]

Replays one golden case through dist/index.js and compares the events and the
rollups with the case's expected files. Writes actual-events.json,
actual-rollup.json and result.json under --out and prints result.json on stdout.

  --out <dir>  directory for the actual output and the result (required)
  --dry-run    print the case directory and the output paths as JSON and exit
  --quiet      no pass/fail line on stderr
  --help       this text

exit codes: 0 pass, 1 events or a rollup differ, 2 usage error

examples:
  golden.mjs claude/basic --out .verify/evidence/20261018T180000Z/golden/claude-basic
  for c in golden/*/*/expected-events.json; do c=\${c#golden/}; golden.mjs \${c%/expected-events.json} --out /tmp/g/\${c%/*} --quiet; done`;
const { positional, one, dryRun, log, usage } = parseArgs(help, ["--out"]);
const [caseId] = positional;
const outDir = one("--out");
if (!caseId?.includes("/") || !outDir)
  usage("golden: <harness>/<case> and --out <dir> are required");

const dir = join(root, "golden", caseId);
const outputs = ["actual-events.json", "actual-rollup.json", "result.json"].map((f) =>
  join(outDir, f),
);
if (dryRun) {
  console.log(JSON.stringify({ case: caseId, dir: relative(root, dir), outputs }, null, 2));
  process.exit(0);
}

const { options, dims, events, by, result } = await replayCase(caseId);
const read = (name) => JSON.parse(readFileSync(join(dir, name), "utf8"));
const expectedEvents = read("expected-events.json");
const expectedRollup = read("expected-rollup.json");
const summary = {
  case: caseId,
  options,
  events: events.length,
  eventsMatch: canonical(events) === canonical(expectedEvents.events),
  rollupsMatch: Object.fromEntries(
    dims.map((d) => [d, canonical(by[d]) === canonical(expectedRollup.by[d])]),
  ),
  diagnostics: result.diagnostics.length,
  skipped: Object.values(result.skipped).flat().length,
};

mkdirSync(outDir, { recursive: true });
writeFileSync(outputs[0], JSON.stringify({ events }, null, 2));
writeFileSync(outputs[1], JSON.stringify({ by }, null, 2));
writeFileSync(outputs[2], JSON.stringify(summary, null, 2));
console.log(JSON.stringify(summary, null, 2));
const pass = summary.eventsMatch && Object.values(summary.rollupsMatch).every(Boolean);
log(pass ? `golden: pass ${caseId}` : `golden: FAIL ${caseId}`);
process.exit(pass ? 0 : 1);

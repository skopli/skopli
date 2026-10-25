#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { canonical, replayCase } from "./replay.mjs";

const [caseId, outFlag, outDir] = process.argv.slice(2);
if (!caseId || outFlag !== "--out" || !outDir) {
  console.error("usage: golden.mjs <harness>/<case> --out <dir>");
  process.exit(2);
}

const { dir, options, dims, events, by, result } = await replayCase(caseId);
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
writeFileSync(join(outDir, "actual-events.json"), JSON.stringify({ events }, null, 2));
writeFileSync(join(outDir, "actual-rollup.json"), JSON.stringify({ by }, null, 2));
writeFileSync(join(outDir, "result.json"), JSON.stringify(summary, null, 2));
console.log(JSON.stringify(summary, null, 2));
const pass = summary.eventsMatch && Object.values(summary.rollupsMatch).every(Boolean);
console.log(pass ? "golden: pass" : "golden: FAIL");
process.exit(pass ? 0 : 1);

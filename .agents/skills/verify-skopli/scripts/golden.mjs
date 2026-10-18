#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const [caseId, outFlag, outDir] = process.argv.slice(2);
if (!caseId || outFlag !== "--out" || !outDir) {
  console.error("usage: golden.mjs <harness>/<case> --out <dir>");
  process.exit(2);
}

const root = resolve(fileURLToPath(import.meta.url), "../../../../..");
const { readUsage, rollup } = await import(join(root, "dist/index.js"));
const dir = join(root, "golden", caseId);
const input = join(dir, "input");
const read = (name) => JSON.parse(readFileSync(join(dir, name), "utf8"));
const manifest = read("input-manifest.json");

const place = (v) => (isAbsolute(v) ? v : v === "." ? input : join(input, v));
const env = Object.fromEntries(Object.entries(manifest.env ?? {}).map(([k, v]) => [k, place(v)]));
const options = {
  home: manifest.home ? place(manifest.home) : "/nonexistent",
  env,
  harnesses: [manifest.harness],
  tz: manifest.options?.tz ?? "UTC",
};
if (manifest.cwd) options.cwd = place(manifest.cwd);

const relativize = (s) => s.replaceAll(input, "").replace(/(^|\s)\/+/g, "$1");
const key = (e) => [e.timestamp, e.sessionId, e.messageId, e.model].join("\0");
const result = await readUsage(options);
const events = result.events
  .map((e) => ({
    ...e,
    sessionId: e.sessionId.startsWith(input) ? relativize(e.sessionId) : e.sessionId,
    messageId: relativize(e.messageId),
  }))
  .sort((a, b) => (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0));
const dims = manifest.rollups ?? ["model", "day", "harness"];
const by = Object.fromEntries(
  dims.map((d) => [d, rollup(events, d === "day" ? { by: d, tz: "UTC" } : { by: d })]),
);

const canonical = (v) =>
  JSON.stringify(v, (_, x) =>
    x && typeof x === "object" && !Array.isArray(x)
      ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : 1)))
      : x,
  );
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

#!/usr/bin/env node
import { mkdirSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const [home, outFlag, outDir] = process.argv.slice(2);
if (!home || outFlag !== "--out" || !outDir) {
  console.error("usage: home.mjs <home-dir> --out <dir>");
  process.exit(2);
}
const dir = resolve(home);
if (!statSync(dir, { throwIfNoEntry: false })?.isDirectory()) {
  console.error(`home.mjs: ${dir} is not a directory`);
  process.exit(2);
}

const root = resolve(fileURLToPath(import.meta.url), "../../../../..");
const { detectHarnesses, readUsage } = await import(join(root, "dist/index.js"));
const options = { home: dir, env: {}, cwd: dir };
const detected = detectHarnesses(options);
const result = await readUsage({ ...options, tz: "UTC" });
const sessions = (events) => new Set(events.map((e) => e.sessionId)).size;
const summary = {
  home: dir,
  detected: detected.supported,
  unsupported: detected.unsupported,
  events: result.events.length,
  sessions: sessions(result.events),
  byHarness: Object.fromEntries(
    detected.supported.map((h) => {
      const events = result.events.filter((e) => e.harness === h);
      return [h, { events: events.length, sessions: sessions(events) }];
    }),
  ),
  diagnostics: result.diagnostics.length,
  firstDiagnostic: result.diagnostics[0]?.message ?? null,
  skipped: Object.values(result.skipped).flat().length,
};

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, "home-read.json"), JSON.stringify(result, null, 2));
writeFileSync(join(outDir, "home-result.json"), JSON.stringify(summary, null, 2));
console.log(JSON.stringify(summary, null, 2));

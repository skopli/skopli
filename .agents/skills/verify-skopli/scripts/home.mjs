#!/usr/bin/env node
// Point the built facade at an arbitrary home directory (a real or copied
// harness home) and report what it detects and reads there. Nothing from the
// process environment leaks in: `env` is empty and `cwd` is the home itself.
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const [home, outFlag, outDir] = process.argv.slice(2);
if (!home || outFlag !== "--out" || !outDir) {
  console.error("usage: home.mjs <home-dir> --out <dir>");
  process.exit(2);
}

const root = resolve(fileURLToPath(import.meta.url), "../../../../..");
const { detectHarnesses, readUsage } = await import(join(root, "dist/index.js"));
const options = { home: resolve(home), env: {}, cwd: resolve(home) };
const detected = detectHarnesses(options);
const result = await readUsage({ ...options, tz: "UTC" });
const summary = {
  home: options.home,
  detected: detected.supported,
  unsupported: detected.unsupported,
  events: result.events.length,
  sessions: new Set(result.events.map((e) => e.sessionId)).size,
  byHarness: Object.fromEntries(
    detected.supported.map((h) => [h, result.events.filter((e) => e.harness === h).length]),
  ),
  diagnostics: result.diagnostics.length,
  firstDiagnostic: result.diagnostics[0]?.message ?? null,
  skipped: Object.values(result.skipped).flat().length,
};

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, "home-events.json"), JSON.stringify(result, null, 2));
writeFileSync(join(outDir, "home-result.json"), JSON.stringify(summary, null, 2));
console.log(JSON.stringify(summary, null, 2));

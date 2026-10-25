#!/usr/bin/env node
// Compare what skopli reads from a capture home with what the gateway logged for the same session.
// Gateway lines are summed per wire shape: chat (prompt_tokens minus cached_tokens as input,
// cached_tokens as cache read), messages (input, cache_read, cache_creation) and responses
// (input_tokens minus input_tokens_details.cached_tokens, cached_tokens as cache read). Lines
// tagged aborted are summed separately because the harness never saw them.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "./cli.mjs";
import { root } from "./replay.mjs";

const help = `usage: reconcile.mjs <home> <harness> <gateway-usage.jsonl> [--dry-run] [--quiet]

Reads <home> through the repository's own readers for <harness>, sums the gateway
log of the same session per wire shape, and prints one JSON object on stdout with
the skopli totals, the gateway totals (aborted lines summed separately) and the
per-field delta.

  --dry-run  print the resolved inputs and the gateway line count as JSON and exit
  --quiet    no verdict line on stderr
  --help     this text

exit codes: 0 input, output, cache read and cache write agree, 1 they differ, 2 usage error

examples:
  reconcile.mjs /tmp/cap/home omp /tmp/cap/gateway-usage.jsonl
  reconcile.mjs /tmp/cap/home jcode /tmp/cap/gateway-usage.jsonl --quiet || echo differs`;
const { positional, dryRun, log: say, usage } = parseArgs(help);
const [home, harness, log] = positional;
if (!home || !harness || !log)
  usage("reconcile: <home>, <harness> and <gateway-usage.jsonl> are required");

const zero = () => ({ input: 0, output: 0, cacheRead: 0, cacheWrite: 0 });
const add = (acc, t) => {
  for (const k of Object.keys(acc)) acc[k] += t[k] ?? 0;
  return acc;
};
const fromLine = ({ usage: u = {} }) => {
  const cached = u.prompt_tokens_details?.cached_tokens ?? u.input_tokens_details?.cached_tokens;
  if (cached !== undefined) {
    const prompt = u.prompt_tokens ?? u.input_tokens ?? 0;
    return {
      input: prompt - cached,
      output: u.completion_tokens ?? u.output_tokens ?? 0,
      cacheRead: cached,
      cacheWrite: 0,
    };
  }
  return {
    input: u.input_tokens ?? 0,
    output: u.output_tokens ?? 0,
    cacheRead: u.cache_read_input_tokens ?? 0,
    cacheWrite: u.cache_creation_input_tokens ?? 0,
  };
};
const lines = readFileSync(log, "utf8")
  .split("\n")
  .filter(Boolean)
  .map((l) => JSON.parse(l));
if (dryRun) {
  console.log(JSON.stringify({ home, harness, log, gatewayLines: lines.length }, null, 2));
  process.exit(0);
}
const { readUsage, rollup } = await import(join(root, "dist/index.js"));
const gateway = lines.filter((l) => !l.aborted).reduce((acc, l) => add(acc, fromLine(l)), zero());
const aborted = lines.filter((l) => l.aborted).reduce((acc, l) => add(acc, fromLine(l)), zero());

const result = await readUsage({ home, harnesses: [harness], tz: "UTC" });
const skopli = add(zero(), rollup(result.events, { by: "harness" })[0]?.tokens ?? {});
const delta = Object.fromEntries(Object.keys(skopli).map((k) => [k, skopli[k] - gateway[k]]));
const agree = Object.values(delta).every((d) => d === 0);
say(agree ? "reconcile: agree" : `reconcile: differ ${JSON.stringify(delta)}`);
console.log(
  JSON.stringify({
    harness,
    events: result.events.length,
    sessions: new Set(result.events.map((e) => e.sessionId)).size,
    gatewayLines: lines.length,
    abortedLines: lines.filter((l) => l.aborted).length,
    skopli,
    gateway,
    aborted,
    delta,
    agree,
  }),
);
process.exit(agree ? 0 : 1);

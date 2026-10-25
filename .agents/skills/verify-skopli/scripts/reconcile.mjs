#!/usr/bin/env node
// Compare what skopli reads from a capture home with what the gateway logged for the same session.
//
//   reconcile.mjs <home> <harness> <gateway-usage.jsonl>
//
// Gateway lines are summed per wire shape: chat (prompt_tokens minus cached_tokens as input,
// cached_tokens as cache read), messages (input, cache_read, cache_creation) and responses
// (input_tokens minus input_tokens_details.cached_tokens, cached_tokens as cache read). Lines
// tagged aborted are summed separately because the harness never saw them. Prints one JSON
// object with both totals and the per-field delta; exit 0 when every field agrees.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { root } from "./replay.mjs";

const [home, harness, log] = process.argv.slice(2);
if (!home || !harness || !log) {
  console.error("usage: reconcile.mjs <home> <harness> <gateway-usage.jsonl>");
  process.exit(2);
}
const { readUsage, rollup } = await import(join(root, "dist/index.js"));

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
const gateway = lines.filter((l) => !l.aborted).reduce((acc, l) => add(acc, fromLine(l)), zero());
const aborted = lines.filter((l) => l.aborted).reduce((acc, l) => add(acc, fromLine(l)), zero());

const result = await readUsage({ home, harnesses: [harness], tz: "UTC" });
const skopli = add(zero(), rollup(result.events, { by: "harness" })[0]?.tokens ?? {});
const delta = Object.fromEntries(Object.keys(skopli).map((k) => [k, skopli[k] - gateway[k]]));
const agree = Object.values(delta).every((d) => d === 0);
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

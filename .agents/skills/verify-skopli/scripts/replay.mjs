// Shared golden replay: resolve a case's manifest, read it through dist/index.js and
// normalise the output the way the expected files store it (used by golden.mjs and promote.mjs).
import { readFileSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const root = resolve(fileURLToPath(import.meta.url), "../../../../..");

export const canonical = (v) =>
  JSON.stringify(v, (_, x) =>
    x && typeof x === "object" && !Array.isArray(x)
      ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : 1)))
      : x,
  );

export const replayCase = async (caseId, dir = join(root, "golden", caseId)) => {
  const { readUsage, rollup } = await import(join(root, "dist/index.js"));
  const input = join(dir, "input");
  const manifest = JSON.parse(readFileSync(join(dir, "input-manifest.json"), "utf8"));

  const place = (v) => (isAbsolute(v) ? v : join(input, v));
  const at = (v) => (v === "." ? input : v);
  const env = Object.fromEntries(Object.entries(manifest.env ?? {}).map(([k, v]) => [k, place(v)]));
  const options = {
    home: manifest.home ? at(manifest.home) : "/nonexistent",
    env,
    harnesses: [manifest.harness],
    tz: manifest.options?.tz ?? "UTC",
  };
  if (manifest.cwd) options.cwd = at(manifest.cwd);

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
  return { dir, options, dims, events, by, result, relativize };
};

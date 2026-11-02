import { readFileSync } from "node:fs";
import { rollup } from "../../../src/rollup.ts";

const cases = JSON.parse(readFileSync(0, "utf8"));
for (const c of cases) {
  for (const e of c.events) if (typeof e.costUsd === "string") e.costUsd = Number(e.costUsd);
}
process.stdout.write(JSON.stringify(cases.map((c) => rollup(c.events, c.options))));

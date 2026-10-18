// Runs the TypeScript rollup over the cases the Rust harness wrote, one result per case.
import { readFileSync } from "node:fs";
import { rollup } from "../../../src/rollup.ts";

const cases = JSON.parse(readFileSync(process.argv[2], "utf8"));
process.stdout.write(JSON.stringify(cases.map((c) => rollup(c.events, c.options))));

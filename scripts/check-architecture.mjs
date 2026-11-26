import { readFileSync } from "node:fs";
import { join } from "node:path";

const rollupFixtures = [
  "function dayKey",
  "function blockRollup",
  "function withinHour",
  "Intl.DateTimeFormat",
  "dateParseMs(",
];

export const rollupFindings = (text) => {
  const findings = [];
  if (!text.includes("native().rollup")) {
    findings.push("src/rollup.ts must delegate to native().rollup");
  }
  for (const fixture of rollupFixtures) {
    if (text.includes(fixture)) {
      findings.push(`src/rollup.ts contains rollup implementation detail ${fixture}`);
    }
  }
  return findings;
};

const main = () => {
  const path = join(import.meta.dirname, "..", "src", "rollup.ts");
  const findings = rollupFindings(readFileSync(path, "utf8"));
  if (findings.length) {
    console.error(findings.join("\n"));
    console.error(
      "architecture: keep rollup math in crates/skopli-core and make the TypeScript facade a thin wire adapter",
    );
    process.exit(1);
  }
  console.log("architecture rules hold: TypeScript rollup delegates to the Rust core");
};

if (process.argv[1] === import.meta.filename) main();

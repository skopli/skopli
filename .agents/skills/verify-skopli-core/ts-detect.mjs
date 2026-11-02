import { readFileSync } from "node:fs";
import { detectHarnesses } from "../../../src/detect.ts";
import { createPathResolver } from "../../../src/paths.ts";

const homes = JSON.parse(readFileSync(0, "utf8"));
process.stdout.write(
  JSON.stringify(
    homes.map((h) => detectHarnesses(createPathResolver({ home: h.home, env: h.env, cwd: h.cwd }))),
  ),
);

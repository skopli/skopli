import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import { readVersions, versionErrors } from "../scripts/check-versions.mjs";

const at = (version: string | undefined, where = "a") => ({ where, version });

describe("check-versions", () => {
  it("finds one shared version in every manifest of this repository", () => {
    expect(versionErrors(readVersions("."), undefined)).toEqual([]);
  });

  it("treats an empty tag argument as no tag", () => {
    const run = spawnSync(process.execPath, ["scripts/check-versions.mjs", ""], {
      encoding: "utf8",
    });
    expect(run.status).toBe(0);
  });

  it("fails with one line per manifest when the tag matches none of them", () => {
    const run = spawnSync(process.execPath, ["scripts/check-versions.mjs", "v9.9.9"], {
      encoding: "utf8",
    });
    expect(run.status).toBe(1);
    expect(run.stderr.trim().split("\n")).toHaveLength(8);
  });

  it("names every manifest that disagrees with the first one", () => {
    const versions = [at("1.0.0", "Cargo.toml"), at("1.0.0", "b"), at("1.0.1", "c")];
    expect(versionErrors(versions, undefined)).toEqual(["c: 1.0.1, but Cargo.toml is 1.0.0"]);
  });

  it("compares every manifest with the tag when one is given", () => {
    expect(versionErrors([at("1.0.0"), at("1.0.0", "b")], "v1.0.0")).toEqual([]);
    expect(versionErrors([at("1.0.0"), at("1.1.0", "b")], "v1.1.0")).toEqual([
      "a: 1.0.0, but tag v1.1.0 is 1.1.0",
    ]);
  });

  it("reports a manifest whose version line is missing", () => {
    expect(versionErrors([at("1.0.0"), at(undefined, "b")], "v1.0.0")).toEqual([
      "b: no version found",
    ]);
  });
});

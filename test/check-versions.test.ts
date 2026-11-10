import { describe, expect, it } from "vitest";
import { locations, readVersions, versionErrors } from "../scripts/check-versions.mjs";

const at = (version: string | undefined, where = "a") => ({ where, version });

describe("check-versions", () => {
  it("finds one shared version in every manifest of this repository", () => {
    const versions = readVersions(".");
    expect(versions).toHaveLength(locations.length);
    expect(versionErrors(versions, undefined)).toEqual([]);
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

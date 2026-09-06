import { homedir } from "node:os";
import { describe, expect, it } from "vitest";
import { createDiagnostics } from "../src/diagnostics.ts";
import { createPathResolver } from "../src/paths.ts";

describe("createPathResolver", () => {
  it("defaults home to the real home directory", () => {
    const resolver = createPathResolver();
    expect(resolver.home()).toBe(homedir());
  });

  it("uses the injected home", () => {
    const resolver = createPathResolver({ home: "/fixtures/home" });
    expect(resolver.home()).toBe("/fixtures/home");
  });

  it("reads process.env by default", () => {
    process.env["SKOPLI_TEST_VAR"] = "value";
    try {
      const resolver = createPathResolver();
      expect(resolver.env("SKOPLI_TEST_VAR")).toBe("value");
    } finally {
      delete process.env["SKOPLI_TEST_VAR"];
    }
  });

  it("treats an injected env map as the entire environment", () => {
    process.env["SKOPLI_TEST_VAR"] = "leaked";
    try {
      const resolver = createPathResolver({ env: { OTHER: "x" } });
      expect(resolver.env("SKOPLI_TEST_VAR")).toBeUndefined();
      expect(resolver.env("OTHER")).toBe("x");
    } finally {
      delete process.env["SKOPLI_TEST_VAR"];
    }
  });

  it("normalizes empty-string env values to undefined", () => {
    const resolver = createPathResolver({ env: { EMPTY: "" } });
    expect(resolver.env("EMPTY")).toBeUndefined();
  });
});

describe("createDiagnostics", () => {
  it("collects warnings with optional harness and path", () => {
    const sink = createDiagnostics();
    sink.warn("skipping malformed line", { harness: "claude", path: "/x.jsonl" });
    sink.add("info", "note");
    expect(sink.list()).toEqual([
      {
        severity: "warning",
        message: "skipping malformed line",
        harness: "claude",
        path: "/x.jsonl",
      },
      { severity: "info", message: "note" },
    ]);
  });

  it("omits absent optional fields entirely", () => {
    const sink = createDiagnostics();
    sink.warn("bare");
    const [entry] = sink.list();
    expect(entry).toBeDefined();
    expect(Object.keys(entry ?? {})).toEqual(["severity", "message"]);
  });
});

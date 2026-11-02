import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const root = resolve(import.meta.dirname, "../../../..");
const script = (name: string) => join(import.meta.dirname, name);
const run = (name: string, ...args: string[]) =>
  spawnSync("node", [script(name), ...args], { encoding: "utf8" });

describe("script command lines", () => {
  it.each(["promote.mjs", "reconcile.mjs", "golden.mjs"])("%s --help", (name) => {
    const { status, stdout } = run(name, "--help");
    expect(status).toBe(0);
    expect(stdout).toMatch(/^usage: /);
    expect(stdout).toContain("examples:");
  });

  it("rejects an unknown option with exit 2", () => {
    const { status, stderr } = run("promote.mjs", "/x", "h/c", "--store", "s", "--bogus");
    expect(status).toBe(2);
    expect(stderr).toContain("--bogus");
  });

  it("promote --dry-run previews longest-first rules and the files, writing nothing", () => {
    const home = mkdtempSync(join(tmpdir(), "promote-dry-"));
    mkdirSync(join(home, "store", "alice-x-y"), { recursive: true });
    writeFileSync(join(home, "store", "alice-x-y", "s.json"), "{}");
    const { status, stdout } = run(
      "promote.mjs",
      home,
      "promote-dry-run/case",
      "--store",
      "store",
      "--redact",
      "alice=user",
      "--redact",
      "/home/alice/x/y=/home/user/project",
      "--dry-run",
    );
    expect(status).toBe(0);
    const plan = JSON.parse(stdout) as { rules: [string, string][]; files: string[] };
    const sources = plan.rules.map(([from]) => from);
    expect(sources.indexOf("/home/alice/x/y")).toBeLessThan(sources.indexOf("alice"));
    const lengths = sources.map((s) => s.length);
    expect(lengths).toEqual([...lengths].sort((a, b) => b - a));
    expect(plan.files).toEqual(["golden/promote-dry-run/case/input/store/user-x-y/s.json"]);
    expect(existsSync(join(root, "golden", "promote-dry-run"))).toBe(false);
  });

  it("promote --cwd points the manifest cwd at input and refuses --env", () => {
    const home = mkdtempSync(join(tmpdir(), "promote-cwd-"));
    mkdirSync(join(home, "trajectories"), { recursive: true });
    writeFileSync(join(home, "trajectories", "t.json"), "{}");
    const plan = (...extra: string[]) =>
      run(
        "promote.mjs",
        home,
        "promote-cwd/case",
        "--store",
        "trajectories",
        "--dry-run",
        ...extra,
      );
    const manifestOf = (stdout: string) =>
      (JSON.parse(stdout) as { manifest: Record<string, unknown> }).manifest;
    expect(manifestOf(plan().stdout)).toMatchObject({ home: "." });
    expect(manifestOf(plan("--cwd").stdout)).toMatchObject({ cwd: "." });
    expect(manifestOf(plan("--cwd").stdout)).not.toHaveProperty("home");
    const both = plan("--cwd", "--env", "X=trajectories");
    expect(both.status).toBe(2);
    expect(both.stderr).toContain("--cwd and --env are exclusive");
  });

  it("promote keeps the existing case when a store is missing or a file fails redaction", () => {
    const home = mkdtempSync(join(tmpdir(), "promote-keep-"));
    mkdirSync(join(home, "store"));
    writeFileSync(join(home, "store", "s.json"), '{"user":"alice"}');
    const existing = join(root, "golden", "promote-keep", "case", "input", "old.json");
    mkdirSync(join(existing, ".."), { recursive: true });
    writeFileSync(existing, "{}");
    try {
      const missing = run("promote.mjs", home, "promote-keep/case", "--store", "nope");
      expect(missing.status).toBe(2);
      expect(missing.stderr).toContain("nope");
      expect(existsSync(existing)).toBe(true);
      const leak = run(
        "promote.mjs",
        home,
        "promote-keep/case",
        "--store",
        "store",
        "--redact",
        "alice=alice2",
      );
      expect(leak.status).toBe(1);
      expect(leak.stderr).toContain("redaction source still present");
      expect(existsSync(existing)).toBe(true);
      expect(existsSync(join(root, ".verify", "promote", "promote-keep", "case"))).toBe(false);
    } finally {
      rmSync(join(root, "golden", "promote-keep"), { recursive: true, force: true });
    }
  });
});

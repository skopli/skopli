import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
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

  it("promote --dry-run previews the rules and the files, writing nothing", () => {
    const home = mkdtempSync(join(tmpdir(), "promote-dry-"));
    mkdirSync(join(home, "store", "ubuntu-x-y"), { recursive: true });
    writeFileSync(join(home, "store", "ubuntu-x-y", "s.json"), "{}");
    const { status, stdout } = run(
      "promote.mjs",
      home,
      "promote-dry-run/case",
      "--store",
      "store",
      "--redact",
      "ubuntu=user",
      "--redact",
      "/home/ubuntu/x/y=/home/user/project",
      "--dry-run",
    );
    expect(status).toBe(0);
    const plan = JSON.parse(stdout) as { rules: [string, string][]; files: string[] };
    expect(plan.rules.map(([from]) => from)).toContain("/home/ubuntu/x/y");
    expect(plan.files).toEqual(["golden/promote-dry-run/case/input/store/user-x-y/s.json"]);
    expect(existsSync(join(root, "golden", "promote-dry-run"))).toBe(false);
  });
});

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { detectHarnesses } from "../src/detect.ts";
import { createPathResolver } from "../src/paths.ts";
import type { Harness } from "../src/types.ts";

// The shared registry-id fixture is the canonical list of harness ids the core
// registers; every id must be assignable to the Harness union (typing this
// tuple as readonly Harness[] fails typecheck if an id is dropped from the
// union) and the union must cover every fixture id at runtime.
const registryIdsPath = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "golden",
  "registry",
  "ids.json",
);

// A record keyed by the full Harness union: TypeScript requires an entry for
// every union member (a dropped id fails the object literal) and rejects any
// key outside the union, so this is the compile-time half of the drift guard.
const KNOWN_HARNESSES = {
  claude: true,
  codex: true,
  gemini: true,
  opencode: true,
  mimocode: true,
  commandcode: true,
  copilot: true,
  amp: true,
  droid: true,
  qwen: true,
  pi: true,
  omp: true,
  prime: true,
  gajae: true,
  kimchi: true,
  grok: true,
  augment: true,
  codebuff: true,
  codebuddy: true,
  jcode: true,
  mux: true,
  zcode: true,
  roo: true,
  cline: true,
  kilo: true,
  kilocode: true,
  openclaw: true,
  kimi: true,
  junie: true,
  devin: true,
  hermes: true,
  goose: true,
  zed: true,
  cherrystudio: true,
  opencodereview: true,
  trae: true,
  deepseek: true,
  reasonix: true,
  kiro: true,
  fx: true,
} satisfies Record<Harness, true>;

describe("Harness union covers the registry-id fixture", () => {
  it("has an entry for every registered id", () => {
    const ids = JSON.parse(readFileSync(registryIdsPath, "utf8")).ids as string[];
    for (const id of ids) {
      expect(Object.hasOwn(KNOWN_HARNESSES, id)).toBe(true);
    }
    expect(Object.keys(KNOWN_HARNESSES).length).toBe(ids.length);
  });
});

// The pi-ai family sessions roots resolve from their env overrides with
// distinct suffixes: GJC_CODING_AGENT_DIR is an agent dir (<dir>/sessions) while
// GJC_CONFIG_DIR is the .gjc config root (<dir>/agent/sessions). Detection is
// the observable surface for these private resolvers, so these tests create the
// resolved directory and assert gajae is detected.

function makeHome(): string {
  return mkdtempSync(join(tmpdir(), "skopli-detect-"));
}

describe("detectHarnesses gajae roots", () => {
  it("resolves GJC_CODING_AGENT_DIR to <dir>/sessions", () => {
    const home = makeHome();
    const agentDir = mkdtempSync(join(tmpdir(), "skopli-gjc-agent-"));
    try {
      mkdirSync(join(agentDir, "sessions"), { recursive: true });
      const resolver = createPathResolver({ home, env: { GJC_CODING_AGENT_DIR: agentDir } });
      expect(detectHarnesses(resolver).supported).toContain("gajae");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(agentDir, { recursive: true, force: true });
    }
  });

  it("resolves GJC_CONFIG_DIR to <dir>/agent/sessions", () => {
    const home = makeHome();
    const configDir = mkdtempSync(join(tmpdir(), "skopli-gjc-config-"));
    try {
      mkdirSync(join(configDir, "agent", "sessions"), { recursive: true });
      const resolver = createPathResolver({ home, env: { GJC_CONFIG_DIR: configDir } });
      expect(detectHarnesses(resolver).supported).toContain("gajae");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(configDir, { recursive: true, force: true });
    }
  });

  it("does not detect GJC_CONFIG_DIR when only <dir>/sessions exists", () => {
    const home = makeHome();
    const configDir = mkdtempSync(join(tmpdir(), "skopli-gjc-config-shallow-"));
    try {
      mkdirSync(join(configDir, "sessions"), { recursive: true });
      const resolver = createPathResolver({ home, env: { GJC_CONFIG_DIR: configDir } });
      expect(detectHarnesses(resolver).supported).not.toContain("gajae");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(configDir, { recursive: true, force: true });
    }
  });

  it("prefers GJC_CODING_AGENT_DIR over GJC_CONFIG_DIR", () => {
    const home = makeHome();
    const agentDir = mkdtempSync(join(tmpdir(), "skopli-gjc-agent-win-"));
    const configDir = mkdtempSync(join(tmpdir(), "skopli-gjc-config-win-"));
    try {
      mkdirSync(join(configDir, "agent", "sessions"), { recursive: true });
      const resolver = createPathResolver({
        home,
        env: { GJC_CODING_AGENT_DIR: agentDir, GJC_CONFIG_DIR: configDir },
      });
      expect(detectHarnesses(resolver).supported).not.toContain("gajae");
      mkdirSync(join(agentDir, "sessions"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("gajae");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(agentDir, { recursive: true, force: true });
      rmSync(configDir, { recursive: true, force: true });
    }
  });

  it("resolves the default home ~/.gjc/agent/sessions", () => {
    const home = makeHome();
    try {
      mkdirSync(join(home, ".gjc", "agent", "sessions"), { recursive: true });
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).toContain("gajae");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("resolves the XDG state candidate $XDG_STATE_HOME/gjc/agent/sessions", () => {
    const home = makeHome();
    const xdgState = mkdtempSync(join(tmpdir(), "skopli-gjc-xdg-"));
    try {
      mkdirSync(join(xdgState, "gjc", "agent", "sessions"), { recursive: true });
      const resolver = createPathResolver({ home, env: { XDG_STATE_HOME: xdgState } });
      expect(detectHarnesses(resolver).supported).toContain("gajae");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(xdgState, { recursive: true, force: true });
    }
  });
});

// MiMo Code is an opencode fork under the `mimocode` app dir. MIMOCODE_HOME
// resolves to <home>/data, XDG_DATA_HOME to <xdg>/mimocode, and LOCALAPPDATA to
// the Windows fallback; detection keys on a `storage/` subdir.

describe("detectHarnesses mimocode roots", () => {
  it("resolves MIMOCODE_HOME to <home>/data/storage", () => {
    const home = makeHome();
    const mimoHome = mkdtempSync(join(tmpdir(), "skopli-mimo-home-"));
    try {
      mkdirSync(join(mimoHome, "data", "storage"), { recursive: true });
      const resolver = createPathResolver({ home, env: { MIMOCODE_HOME: mimoHome } });
      expect(detectHarnesses(resolver).supported).toContain("mimocode");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(mimoHome, { recursive: true, force: true });
    }
  });

  it("resolves XDG_DATA_HOME to <xdg>/mimocode/storage", () => {
    const home = makeHome();
    const xdg = mkdtempSync(join(tmpdir(), "skopli-mimo-xdg-"));
    try {
      mkdirSync(join(xdg, "mimocode", "storage"), { recursive: true });
      const resolver = createPathResolver({ home, env: { XDG_DATA_HOME: xdg } });
      expect(detectHarnesses(resolver).supported).toContain("mimocode");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(xdg, { recursive: true, force: true });
    }
  });

  it("prefers MIMOCODE_HOME over XDG_DATA_HOME", () => {
    const home = makeHome();
    const mimoHome = mkdtempSync(join(tmpdir(), "skopli-mimo-home-win-"));
    const xdg = mkdtempSync(join(tmpdir(), "skopli-mimo-xdg-win-"));
    try {
      // Only the XDG candidate exists: MIMOCODE_HOME wins so nothing resolves.
      mkdirSync(join(xdg, "mimocode", "storage"), { recursive: true });
      const resolver = createPathResolver({
        home,
        env: { MIMOCODE_HOME: mimoHome, XDG_DATA_HOME: xdg },
      });
      expect(detectHarnesses(resolver).supported).not.toContain("mimocode");
      // Now the MIMOCODE_HOME candidate exists too and resolution succeeds.
      mkdirSync(join(mimoHome, "data", "storage"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("mimocode");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(mimoHome, { recursive: true, force: true });
      rmSync(xdg, { recursive: true, force: true });
    }
  });

  it("resolves the default home ~/.local/share/mimocode", () => {
    const home = makeHome();
    try {
      mkdirSync(join(home, ".local", "share", "mimocode", "storage"), { recursive: true });
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).toContain("mimocode");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});

// Command Code is an opencode fork under the `commandcode` app dir.
// XDG_DATA_HOME resolves to <xdg>/commandcode, and the default candidate set
// covers the per-OS data dirs used by Command Code plus the Windows LOCALAPPDATA
// candidate; detection keys on a `storage/` subdir.

describe("detectHarnesses commandcode roots", () => {
  it("resolves XDG_DATA_HOME to <xdg>/commandcode/storage", () => {
    const home = makeHome();
    const xdg = mkdtempSync(join(tmpdir(), "skopli-cc-xdg-"));
    try {
      mkdirSync(join(xdg, "commandcode", "storage"), { recursive: true });
      const resolver = createPathResolver({ home, env: { XDG_DATA_HOME: xdg } });
      expect(detectHarnesses(resolver).supported).toContain("commandcode");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(xdg, { recursive: true, force: true });
    }
  });

  it("prefers XDG_DATA_HOME over the default per-OS candidates", () => {
    const home = makeHome();
    const xdg = mkdtempSync(join(tmpdir(), "skopli-cc-xdg-win-"));
    try {
      // Only the default macOS candidate exists: XDG wins so nothing resolves.
      mkdirSync(join(home, "Library", "Application Support", "CommandCode", "storage"), {
        recursive: true,
      });
      const resolver = createPathResolver({ home, env: { XDG_DATA_HOME: xdg } });
      expect(detectHarnesses(resolver).supported).not.toContain("commandcode");
      // Now the XDG candidate exists too and resolution succeeds.
      mkdirSync(join(xdg, "commandcode", "storage"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("commandcode");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(xdg, { recursive: true, force: true });
    }
  });

  it("resolves the LOCALAPPDATA candidate <local>/commandcode/storage", () => {
    const home = makeHome();
    const local = mkdtempSync(join(tmpdir(), "skopli-cc-local-"));
    try {
      mkdirSync(join(local, "commandcode", "storage"), { recursive: true });
      const resolver = createPathResolver({ home, env: { LOCALAPPDATA: local } });
      expect(detectHarnesses(resolver).supported).toContain("commandcode");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(local, { recursive: true, force: true });
    }
  });

  it("resolves the default macOS candidate ~/Library/Application Support/CommandCode", () => {
    const home = makeHome();
    try {
      mkdirSync(join(home, "Library", "Application Support", "CommandCode", "storage"), {
        recursive: true,
      });
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).toContain("commandcode");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});

// CodeBuddy Code is a Claude Code transcript clone under ~/.codebuddy with the
// CODEBUDDY_CONFIG_DIR override; detection keys on a `projects/` subdir. It must
// stay distinct from the unrelated `codebuff` reader.

describe("detectHarnesses codebuddy roots", () => {
  it("resolves the default home ~/.codebuddy/projects", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("codebuddy");
      mkdirSync(join(home, ".codebuddy", "projects"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("codebuddy");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("resolves CODEBUDDY_CONFIG_DIR to <dir>/projects", () => {
    const home = makeHome();
    const configDir = mkdtempSync(join(tmpdir(), "skopli-codebuddy-"));
    try {
      const resolver = createPathResolver({ home, env: { CODEBUDDY_CONFIG_DIR: configDir } });
      expect(detectHarnesses(resolver).supported).not.toContain("codebuddy");
      mkdirSync(join(configDir, "projects"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("codebuddy");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(configDir, { recursive: true, force: true });
    }
  });

  it("keeps codebuddy distinct from codebuff", () => {
    const home = makeHome();
    try {
      mkdirSync(join(home, ".config", "manicode", "projects"), { recursive: true });
      const resolver = createPathResolver({ home, env: {} });
      const supported = detectHarnesses(resolver).supported;
      expect(supported).toContain("codebuff");
      expect(supported).not.toContain("codebuddy");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});

// jcode stores per-session snapshots + journals under ~/.jcode/sessions/ with
// the JCODE_HOME override; detection keys on the sessions dir.

describe("detectHarnesses jcode roots", () => {
  it("resolves the default home ~/.jcode/sessions", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("jcode");
      mkdirSync(join(home, ".jcode", "sessions"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("jcode");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("resolves JCODE_HOME to <dir>/sessions", () => {
    const home = makeHome();
    const jcodeHome = mkdtempSync(join(tmpdir(), "skopli-jcode-"));
    try {
      const resolver = createPathResolver({ home, env: { JCODE_HOME: jcodeHome } });
      expect(detectHarnesses(resolver).supported).not.toContain("jcode");
      mkdirSync(join(jcodeHome, "sessions"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("jcode");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(jcodeHome, { recursive: true, force: true });
    }
  });
});

describe("detectHarnesses opencodereview roots", () => {
  it("resolves the default home ~/.opencodereview/sessions", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("opencodereview");
      mkdirSync(join(home, ".opencodereview", "sessions"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("opencodereview");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});

describe("detectHarnesses trae roots", () => {
  it("resolves <cwd>/trajectories", () => {
    const home = makeHome();
    const cwd = mkdtempSync(join(tmpdir(), "skopli-trae-"));
    try {
      const resolver = createPathResolver({ home, env: {}, cwd });
      expect(detectHarnesses(resolver).supported).not.toContain("trae");
      mkdirSync(join(cwd, "trajectories"), { recursive: true });
      expect(detectHarnesses(resolver).supported).toContain("trae");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("yields no trae detection when cwd has no trajectories dir", () => {
    const home = makeHome();
    const cwd = mkdtempSync(join(tmpdir(), "skopli-trae-empty-"));
    try {
      const resolver = createPathResolver({ home, env: {}, cwd });
      expect(detectHarnesses(resolver).supported).not.toContain("trae");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});

describe("detectHarnesses deepseek roots", () => {
  it("detects a SQLite-only DSH_HOME with no JSONL event log", () => {
    const home = makeHome();
    const dsh = mkdtempSync(join(tmpdir(), "skopli-dsh-sqlite-"));
    try {
      const resolver = createPathResolver({ home, env: { DSH_HOME: dsh } });
      expect(detectHarnesses(resolver).supported).not.toContain("deepseek");
      mkdirSync(join(dsh, "sessions", "db"), { recursive: true });
      writeFileSync(join(dsh, "sessions", "db", "sessions.db"), "");
      expect(detectHarnesses(resolver).supported).toContain("deepseek");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(dsh, { recursive: true, force: true });
    }
  });

  it("still detects the JSONL event-log backend", () => {
    const home = makeHome();
    const dsh = mkdtempSync(join(tmpdir(), "skopli-dsh-jsonl-"));
    try {
      const sessionDir = join(dsh, "sessions", "--proj--", "sess");
      mkdirSync(sessionDir, { recursive: true });
      writeFileSync(join(sessionDir, "session.jsonl"), "");
      const resolver = createPathResolver({ home, env: { DSH_HOME: dsh } });
      expect(detectHarnesses(resolver).supported).toContain("deepseek");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(dsh, { recursive: true, force: true });
    }
  });
});

// Kiro CLI writes the current build's store at ~/.kiro/data.sqlite3, and reads
// the legacy ancestor store at data_local_dir/amazon-q/data.sqlite3 for in-place
// upgrades (Windows %LOCALAPPDATA%, macOS ~/Library/Application Support, Linux
// $XDG_DATA_HOME or ~/.local/share). Detection is the observable surface for the
// private path resolver, so each test creates one resolved store and asserts
// kiro is detected there and nowhere by default.

describe("detectHarnesses kiro roots", () => {
  it("is absent when no store exists", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("kiro");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("detects the current ~/.kiro/data.sqlite3 store", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("kiro");
      mkdirSync(join(home, ".kiro"), { recursive: true });
      writeFileSync(join(home, ".kiro", "data.sqlite3"), "");
      expect(detectHarnesses(resolver).supported).toContain("kiro");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("detects the Windows legacy %LOCALAPPDATA%/amazon-q store", () => {
    const home = makeHome();
    const local = mkdtempSync(join(tmpdir(), "skopli-kiro-local-"));
    try {
      const resolver = createPathResolver({ home, env: { LOCALAPPDATA: local } });
      expect(detectHarnesses(resolver).supported).not.toContain("kiro");
      mkdirSync(join(local, "amazon-q"), { recursive: true });
      writeFileSync(join(local, "amazon-q", "data.sqlite3"), "");
      expect(detectHarnesses(resolver).supported).toContain("kiro");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(local, { recursive: true, force: true });
    }
  });

  it("detects the macOS legacy ~/Library/Application Support/amazon-q store", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("kiro");
      mkdirSync(join(home, "Library", "Application Support", "amazon-q"), { recursive: true });
      writeFileSync(join(home, "Library", "Application Support", "amazon-q", "data.sqlite3"), "");
      expect(detectHarnesses(resolver).supported).toContain("kiro");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("detects the Linux legacy $XDG_DATA_HOME/amazon-q store", () => {
    const home = makeHome();
    const xdg = mkdtempSync(join(tmpdir(), "skopli-kiro-xdg-"));
    try {
      const resolver = createPathResolver({ home, env: { XDG_DATA_HOME: xdg } });
      expect(detectHarnesses(resolver).supported).not.toContain("kiro");
      mkdirSync(join(xdg, "amazon-q"), { recursive: true });
      writeFileSync(join(xdg, "amazon-q", "data.sqlite3"), "");
      expect(detectHarnesses(resolver).supported).toContain("kiro");
    } finally {
      rmSync(home, { recursive: true, force: true });
      rmSync(xdg, { recursive: true, force: true });
    }
  });
});

// fx (vercel-labs/fx) persists per-session usage at
// ~/.fx/sessions/<session_id>/usage-v2.json. The profile root constant is ".fx"
// joined to the OS home with no environment override, so the sessions directory
// resolves from home only. Detection probes for a usage-v2.json sidecar beneath
// the sessions directory.
describe("detectHarnesses fx roots", () => {
  it("is absent when no sessions directory exists", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("fx");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("detects a ~/.fx/sessions/<id>/usage-v2.json sidecar", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      expect(detectHarnesses(resolver).supported).not.toContain("fx");
      const session = join(
        home,
        ".fx",
        "sessions",
        "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
      );
      mkdirSync(session, { recursive: true });
      writeFileSync(join(session, "usage-v2.json"), "");
      expect(detectHarnesses(resolver).supported).toContain("fx");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("is absent when the sessions directory has no sidecar", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      mkdirSync(
        join(home, ".fx", "sessions", "1770000000000-1770000000000000000-a1b2c3d4e5f60718"),
        { recursive: true },
      );
      expect(detectHarnesses(resolver).supported).not.toContain("fx");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("is absent when the only sidecar is nested deeper than one level", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      const nested = join(
        home,
        ".fx",
        "sessions",
        "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
        "deeper",
      );
      mkdirSync(nested, { recursive: true });
      writeFileSync(join(nested, "usage-v2.json"), "");
      expect(detectHarnesses(resolver).supported).not.toContain("fx");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("is absent when usage-v2.json is a directory, not a file", () => {
    const home = makeHome();
    try {
      const resolver = createPathResolver({ home, env: {} });
      const session = join(
        home,
        ".fx",
        "sessions",
        "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
      );
      mkdirSync(join(session, "usage-v2.json"), { recursive: true });
      expect(detectHarnesses(resolver).supported).not.toContain("fx");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});

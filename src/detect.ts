// Harness detection stays in the TS runtime facade: unlike reading/parsing
// usage data - which the napi addon now owns - detection is pure directory
// probing (env-override path resolution + existence checks) plus the curated
// `unsupported` list (Cursor/aider/Crush/Duo) that the Rust core
// deliberately does not model. Keeping it here preserves exact
// `detectHarnesses()` parity for consumers.
//
// The per-harness path-root helpers are inlined here so this module is
// self-contained and does NOT import the retired `src/readers/*` layer. Each
// helper is the verbatim path-resolution logic from its former reader module.

import { existsSync, readdirSync, statSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { defaultPathResolver, type PathResolver } from "./paths.ts";
import type { Harness } from "./types.ts";

export type Detection = {
  supported: Harness[];
  unsupported: string[];
};

// --- filesystem probes (were src/readers/shared.ts) ---

function dirExists(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

function listFiles(dir: string, match: (name: string) => boolean): string[] {
  const out: string[] = [];
  const stack = [dir];
  while (stack.length > 0) {
    const current = stack.pop();
    if (current === undefined) break;
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const full = join(current, entry.name);
      if (entry.isDirectory()) stack.push(full);
      else if (entry.isFile() && match(entry.name)) out.push(full);
    }
  }
  return out.sort();
}

// --- per-harness data-root resolvers (were src/readers/<harness>.ts) ---

function ampThreadsRoot(resolver: PathResolver): string {
  const override = resolver.env("AMP_DATA_DIR");
  if (override !== undefined && override !== "") return join(override, "threads");
  return join(resolver.home(), ".local", "share", "amp", "threads");
}

function augmentSessionsRoot(resolver: PathResolver): string {
  const override = resolver.env("AUGMENT_SESSIONS_DIR");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".augment", "sessions");
}

function cherrystudioDbPaths(resolver: PathResolver): string[] {
  const home = resolver.home();
  const paths = [
    join(home, ".config", "CherryStudio", "Data", "cherrystudio.sqlite"),
    join(home, "Library", "Application Support", "CherryStudio", "Data", "cherrystudio.sqlite"),
  ];
  const appData = resolver.env("APPDATA");
  if (appData !== undefined && appData.trim() !== "") {
    paths.push(join(appData.trim(), "CherryStudio", "Data", "cherrystudio.sqlite"));
  }
  return paths;
}

function claudeRoots(resolver: PathResolver): string[] {
  const override = resolver.env("CLAUDE_CONFIG_DIR");
  if (override !== undefined && override !== "") {
    return override
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part !== "");
  }
  return [join(resolver.home(), ".claude"), join(resolver.home(), ".config", "claude")];
}

function codebuffRoots(resolver: PathResolver): string[] {
  const override = resolver.env("CODEBUFF_DATA_DIR");
  const bases =
    override !== undefined && override !== ""
      ? override
          .split(",")
          .map((part) => part.trim())
          .filter((part) => part !== "")
      : [
          join(resolver.home(), ".config", "manicode"),
          join(resolver.home(), ".config", "manicode-dev"),
          join(resolver.home(), ".config", "manicode-staging"),
        ];
  return bases.map((base) => (basename(base) === "projects" ? base : join(base, "projects")));
}

function codebuddyRoots(resolver: PathResolver): string[] {
  const override = resolver.env("CODEBUDDY_CONFIG_DIR");
  if (override !== undefined && override !== "") {
    return [override];
  }
  return [join(resolver.home(), ".codebuddy")];
}

function jcodeSessionsRoot(resolver: PathResolver): string {
  const override = resolver.env("JCODE_HOME");
  if (override !== undefined && override !== "") return join(override, "sessions");
  return join(resolver.home(), ".jcode", "sessions");
}

function devinDbPath(resolver: PathResolver): string {
  const override = resolver.env("DEVIN_DATA_DIR");
  if (override !== undefined && override !== "") {
    return join(override, "cli", "sessions.db");
  }
  return join(resolver.home(), ".local", "share", "devin", "cli", "sessions.db");
}

function devinDesktopRoot(resolver: PathResolver): string {
  const override = resolver.env("DEVIN_DESKTOP_DIR");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), "Library", "Application Support", "Devin", "User", "acp-events");
}

type ExtHarness = "roo" | "cline" | "kilocode";

const EXT_SUFFIX: Record<ExtHarness, string> = {
  roo: join("rooveterinaryinc.roo-cline", "tasks"),
  cline: join("saoudrizwan.claude-dev", "tasks"),
  kilocode: join("kilocode.kilo-code", "tasks"),
};

function vscodeTaskRoots(harness: ExtHarness, resolver: PathResolver): string[] {
  const home = resolver.home();
  const parts = EXT_SUFFIX[harness].split(/[\\/]/);
  const roots = [
    join(home, ".config", "Code", "User", "globalStorage", ...parts),
    join(home, "Library", "Application Support", "Code", "User", "globalStorage", ...parts),
    join(home, ".vscode-server", "data", "User", "globalStorage", ...parts),
    join(home, "AppData", "Roaming", "Code", "User", "globalStorage", ...parts),
  ];
  const appData = resolver.env("APPDATA");
  if (appData !== undefined && appData !== "") {
    roots.push(join(appData, "Code", "User", "globalStorage", ...parts));
  }
  return roots;
}

function rooTaskRoots(resolver: PathResolver): string[] {
  return vscodeTaskRoots("roo", resolver);
}

function clineTaskRoots(resolver: PathResolver): string[] {
  return vscodeTaskRoots("cline", resolver);
}

function kilocodeTaskRoots(resolver: PathResolver): string[] {
  return vscodeTaskRoots("kilocode", resolver);
}

function clineCliSessionRoots(resolver: PathResolver): string[] {
  const direct = resolver.env("CLINE_SESSION_DATA_DIR");
  if (direct !== undefined && direct.trim() !== "") return [direct.trim()];
  const dataDir = resolver.env("CLINE_DATA_DIR");
  if (dataDir !== undefined && dataDir.trim() !== "") return [join(dataDir.trim(), "sessions")];
  const clineDir = resolver.env("CLINE_DIR");
  if (clineDir !== undefined && clineDir.trim() !== "") {
    return [join(clineDir.trim(), "data", "sessions")];
  }
  return [join(resolver.home(), ".cline", "data", "sessions")];
}

function zcodeRoot(resolver: PathResolver): string {
  const override = resolver.env("ZCODE_DATA_DIR");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".zcode");
}

function zcodeDbPath(root: string): string {
  return join(root, "cli", "db", "db.sqlite");
}

function zcodeProjectsRoot(root: string): string {
  return join(root, "projects");
}

function zedDbPaths(resolver: PathResolver): string[] {
  const override = resolver.env("ZED_DATA_DIR");
  if (override !== undefined && override.trim() !== "") {
    return [join(override.trim(), "threads", "threads.db")];
  }
  const home = resolver.home();
  const local = resolver.env("LOCALAPPDATA");
  const paths = [
    join(home, ".local", "share", "zed", "threads", "threads.db"),
    join(home, "Library", "Application Support", "Zed", "threads", "threads.db"),
  ];
  if (local !== undefined && local !== "") {
    paths.push(join(local, "Zed", "threads", "threads.db"));
  }
  return paths;
}

function copilotOtelRoot(resolver: PathResolver): string {
  const override = resolver.env("COPILOT_OTEL_FILE_EXPORTER_PATH");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".copilot", "otel");
}

function codexRoot(resolver: PathResolver): string {
  const override = resolver.env("CODEX_HOME");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".codex");
}

function droidSessionsRoot(resolver: PathResolver): string {
  const override = resolver.env("DROID_SESSIONS_DIR");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".factory", "sessions");
}

function geminiRoot(resolver: PathResolver): string {
  const override = resolver.env("GEMINI_DATA_DIR");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".gemini");
}

function gooseDbPaths(resolver: PathResolver): string[] {
  const override = resolver.env("GOOSE_PATH_ROOT");
  if (override !== undefined && override.trim() !== "") {
    return [join(override.trim(), "data", "sessions", "sessions.db")];
  }
  const home = resolver.home();
  return [
    join(home, ".local", "share", "goose", "sessions", "sessions.db"),
    join(home, "Library", "Application Support", "goose", "sessions", "sessions.db"),
    join(home, ".local", "share", "Block", "goose", "sessions", "sessions.db"),
  ];
}

function grokHome(resolver: PathResolver): string {
  const override = resolver.env("GROK_HOME");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".grok");
}

function hermesHomes(resolver: PathResolver): string[] {
  const override = resolver.env("HERMES_HOME");
  if (override !== undefined && override !== "") {
    return override
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part !== "");
  }
  return [join(resolver.home(), ".hermes")];
}

function hermesDbPaths(homes: string[]): string[] {
  const paths: string[] = [];
  for (const home of homes) {
    if (basename(dirname(home)) === "profiles") {
      paths.push(join(home, "state.db"));
      continue;
    }
    paths.push(join(home, "state.db"));
    try {
      const profilesDir = join(home, "profiles");
      for (const entry of readdirSync(profilesDir, { withFileTypes: true })) {
        if (entry.isDirectory()) paths.push(join(profilesDir, entry.name, "state.db"));
      }
    } catch {
      // no profiles dir
    }
  }
  return [...new Set(paths)];
}

function junieSessionsRoot(resolver: PathResolver): string {
  const override = resolver.env("JUNIE_SESSIONS_DIR");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".junie", "sessions");
}

function kiloDbPath(resolver: PathResolver): string {
  const override = resolver.env("KILO_DATA_DIR");
  if (override !== undefined && override !== "") return join(override, "kilo.db");
  return join(resolver.home(), ".local", "share", "kilo", "kilo.db");
}

function kimiRoots(resolver: PathResolver): string[] {
  const override = resolver.env("KIMI_DATA_DIR");
  if (override !== undefined && override !== "") {
    return override
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part !== "");
  }
  return [join(resolver.home(), ".kimi"), join(resolver.home(), ".kimi-code")];
}

function muxSessionsRoot(resolver: PathResolver): string {
  const override = resolver.env("MUX_SESSIONS_DIR");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".mux", "sessions");
}

function openclawRoots(resolver: PathResolver): string[] {
  const override = resolver.env("OPENCLAW_DIR");
  if (override !== undefined && override !== "") {
    return override
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part !== "");
  }
  const home = resolver.home();
  return [
    join(home, ".openclaw"),
    join(home, ".clawdbot"),
    join(home, ".moltbot"),
    join(home, ".moldbot"),
  ];
}

function opencodeRoots(resolver: PathResolver): string[] {
  const override = resolver.env("OPENCODE_DATA_DIR");
  if (override !== undefined && override !== "") {
    return override
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part !== "");
  }
  const xdg = resolver.env("XDG_DATA_HOME");
  if (xdg !== undefined && xdg !== "") return [join(xdg, "opencode")];
  return [join(resolver.home(), ".local", "share", "opencode")];
}

function mimocodeRoots(resolver: PathResolver): string[] {
  const home = resolver.env("MIMOCODE_HOME");
  if (home !== undefined && home !== "") return [join(home, "data")];
  const xdg = resolver.env("XDG_DATA_HOME");
  if (xdg !== undefined && xdg !== "") return [join(xdg, "mimocode")];
  const local = resolver.env("LOCALAPPDATA");
  if (local !== undefined && local !== "") return [join(local, "mimocode")];
  return [join(resolver.home(), ".local", "share", "mimocode")];
}

// Command Code resolves the XDG data dir first, then the per-OS data dirs used
// by Command Code (Linux ~/.local/share/commandcode, macOS ~/Library/
// Application Support/CommandCode, Windows %LOCALAPPDATA%\commandcode).
function commandcodeRoots(resolver: PathResolver): string[] {
  const xdg = resolver.env("XDG_DATA_HOME");
  if (xdg !== undefined && xdg !== "") return [join(xdg, "commandcode")];
  const roots = [
    join(resolver.home(), ".local", "share", "commandcode"),
    join(resolver.home(), "Library", "Application Support", "CommandCode"),
  ];
  const local = resolver.env("LOCALAPPDATA");
  if (local !== undefined && local !== "") roots.push(join(local, "commandcode"));
  return roots;
}

function piSessionsRoot(resolver: PathResolver): string {
  const override = resolver.env("PI_AGENT_DIR");
  if (override !== undefined && override !== "") return join(override, "sessions");
  return join(resolver.home(), ".pi", "agent", "sessions");
}

function ompSessionsRoot(resolver: PathResolver): string {
  return join(resolver.home(), ".omp", "agent", "sessions");
}

function primeSessionsRoots(resolver: PathResolver): string[] {
  const sessionDir = resolver.env("PRIME_AGENT_SESSION_DIR");
  if (sessionDir !== undefined && sessionDir !== "") return [sessionDir];
  const agentDir = resolver.env("PRIME_AGENT_CODING_AGENT_DIR");
  if (agentDir !== undefined && agentDir !== "") return [join(agentDir, "sessions")];
  return [join(resolver.home(), ".prime", "agent", "sessions")];
}

function gajaeSessionsRoots(resolver: PathResolver): string[] {
  const agentDir = resolver.env("GJC_CODING_AGENT_DIR");
  if (agentDir !== undefined && agentDir !== "") return [join(agentDir, "sessions")];
  const configDir = resolver.env("GJC_CONFIG_DIR");
  if (configDir !== undefined && configDir !== "") return [join(configDir, "agent", "sessions")];
  const roots = [join(resolver.home(), ".gjc", "agent", "sessions")];
  const xdgState = resolver.env("XDG_STATE_HOME");
  if (xdgState !== undefined && xdgState !== "") {
    roots.push(join(xdgState, "gjc", "agent", "sessions"));
  }
  return roots;
}

function kimchiSessionsRoots(resolver: PathResolver): string[] {
  const configPath = resolver.env("KIMCHI_CONFIG_PATH");
  if (configPath !== undefined && configPath !== "") {
    return [join(configPath, "agent", "sessions")];
  }
  return [
    join(resolver.home(), ".config", "kimchi", "agent", "sessions"),
    join(resolver.home(), ".pi", "agent", "sessions"),
  ];
}

function qwenProjectsRoot(resolver: PathResolver): string {
  const override = resolver.env("QWEN_DATA_DIR");
  if (override !== undefined && override !== "") return join(override, "projects");
  return join(resolver.home(), ".qwen", "projects");
}

function opencodereviewSessionsRoot(resolver: PathResolver): string {
  return join(resolver.home(), ".opencodereview", "sessions");
}

function traeTrajectoriesRoot(resolver: PathResolver): string | undefined {
  const cwd = resolver.cwd();
  if (cwd === undefined || cwd === "") return undefined;
  return join(cwd, "trajectories");
}

function deepseekRoot(resolver: PathResolver): string {
  const override = resolver.env("DSH_HOME");
  if (override !== undefined && override !== "") return override;
  return join(resolver.home(), ".dsh");
}

function reasonixSessionsGlobRoot(resolver: PathResolver): string {
  const override = resolver.env("REASONIX_HOME");
  if (override !== undefined && override !== "") return join(override, "projects");
  const appData = resolver.env("APPDATA");
  if (appData !== undefined && appData !== "") return join(appData, "reasonix", "projects");
  return join(resolver.home(), ".reasonix", "projects");
}

// Kiro CLI writes the current build's store at ~/.kiro/data.sqlite3, with the
// legacy ancestor store at data_local_dir/amazon-q/data.sqlite3 (Windows
// %LOCALAPPDATA%, macOS ~/Library/Application Support, Linux $XDG_DATA_HOME or
// ~/.local/share) read for in-place upgrades.
// fx (vercel-labs/fx) persists per-session usage at
// ~/.fx/sessions/<session_id>/usage-v2.json. The profile root constant is
// ".fx" joined to the OS home with no environment override, so the sessions
// directory resolves from home only.
function fxSessionsRoot(resolver: PathResolver): string {
  return join(resolver.home(), ".fx", "sessions");
}

function kiroDbPaths(resolver: PathResolver): string[] {
  const home = resolver.home();
  const paths = [join(home, ".kiro", "data.sqlite3")];
  const local = resolver.env("LOCALAPPDATA");
  if (local !== undefined && local.trim() !== "") {
    paths.push(join(local.trim(), "amazon-q", "data.sqlite3"));
  }
  paths.push(join(home, "Library", "Application Support", "amazon-q", "data.sqlite3"));
  const xdg = resolver.env("XDG_DATA_HOME");
  const linuxData =
    xdg !== undefined && xdg.trim() !== "" ? xdg.trim() : join(home, ".local", "share");
  paths.push(join(linuxData, "amazon-q", "data.sqlite3"));
  return paths;
}

// --- detection ---

function hasAiderFiles(home: string): boolean {
  try {
    return readdirSync(home).some((name) => name.startsWith(".aider"));
  } catch {
    return false;
  }
}

function hasFileWithSuffix(dir: string, suffix: string): boolean {
  if (!dirExists(dir)) return false;
  return listFiles(dir, (name) => name.endsWith(suffix)).length > 0;
}

// Depth-one probe matching the fx reader's discovery contract: a fixed file name
// directly under an immediate child directory of `dir`. Unlike the recursive
// listFiles, this only inspects `<dir>/<child>/<fileName>`, so a nested file
// deeper than one level does not falsely detect the harness.
function hasImmediateChildFile(dir: string, fileName: string): boolean {
  if (!dirExists(dir)) return false;
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return false;
  }
  return entries.some((entry) => {
    if (!entry.isDirectory()) return false;
    try {
      return statSync(join(dir, entry.name, fileName)).isFile();
    } catch {
      return false;
    }
  });
}

function duoDetected(home: string, resolver: PathResolver): boolean {
  const candidates: string[] = [];
  const appData = resolver.env("APPDATA");
  if (appData !== undefined && appData !== "") candidates.push(join(appData, "GitLab", "duo"));
  const xdg = resolver.env("XDG_CONFIG_HOME");
  candidates.push(
    xdg !== undefined && xdg !== ""
      ? join(xdg, "gitlab", "duo")
      : join(home, ".config", "gitlab", "duo"),
  );
  return candidates.some((dir) => dirExists(dir));
}

function crushDetected(home: string, resolver: PathResolver): boolean {
  const xdg = resolver.env("XDG_DATA_HOME");
  const candidates = [
    xdg !== undefined && xdg !== "" ? join(xdg, "crush") : join(home, ".local", "share", "crush"),
    join(home, "AppData", "Local", "crush"),
  ];
  const local = resolver.env("LOCALAPPDATA");
  if (local !== undefined && local !== "") candidates.push(join(local, "crush"));
  return candidates.some((dir) => existsSync(join(dir, "projects.json")));
}

function hasOpencodeDatabase(root: string): boolean {
  try {
    return readdirSync(root).some(
      (name) => name === "opencode.db" || /^opencode-.+\.db$/.test(name),
    );
  } catch {
    return false;
  }
}

export function detectHarnesses(resolver: PathResolver = defaultPathResolver): Detection {
  const supported: Harness[] = [];
  if (claudeRoots(resolver).some((root) => dirExists(join(root, "projects")))) {
    supported.push("claude");
  }
  if (
    opencodeRoots(resolver).some(
      (root) => dirExists(join(root, "storage")) || hasOpencodeDatabase(root),
    )
  ) {
    supported.push("opencode");
  }
  if (
    mimocodeRoots(resolver).some(
      (root) => dirExists(join(root, "storage")) || hasOpencodeDatabase(root),
    )
  ) {
    supported.push("mimocode");
  }
  if (
    commandcodeRoots(resolver).some(
      (root) => dirExists(join(root, "storage")) || hasOpencodeDatabase(root),
    )
  ) {
    supported.push("commandcode");
  }
  if (
    dirExists(join(codexRoot(resolver), "sessions")) ||
    dirExists(join(codexRoot(resolver), "archived_sessions"))
  ) {
    supported.push("codex");
  }
  if (dirExists(join(geminiRoot(resolver), "tmp"))) supported.push("gemini");
  if (existsSync(copilotOtelRoot(resolver))) supported.push("copilot");
  if (dirExists(ampThreadsRoot(resolver))) supported.push("amp");
  if (dirExists(droidSessionsRoot(resolver))) supported.push("droid");
  if (dirExists(qwenProjectsRoot(resolver))) supported.push("qwen");
  if (dirExists(piSessionsRoot(resolver))) supported.push("pi");
  if (dirExists(ompSessionsRoot(resolver))) supported.push("omp");
  if (primeSessionsRoots(resolver).some((root) => dirExists(root))) supported.push("prime");
  if (gajaeSessionsRoots(resolver).some((root) => dirExists(root))) supported.push("gajae");
  if (kimchiSessionsRoots(resolver).some((root) => dirExists(root))) supported.push("kimchi");
  if (existsSync(kiloDbPath(resolver))) supported.push("kilo");
  if (gooseDbPaths(resolver).some((path) => existsSync(path))) supported.push("goose");
  if (hermesDbPaths(hermesHomes(resolver)).some((path) => existsSync(path))) {
    supported.push("hermes");
  }
  if (kimiRoots(resolver).some((root) => dirExists(join(root, "sessions")))) {
    supported.push("kimi");
  }
  if (openclawRoots(resolver).some((root) => dirExists(root))) supported.push("openclaw");
  if (dirExists(junieSessionsRoot(resolver))) supported.push("junie");
  if (
    dirExists(join(grokHome(resolver), "sessions")) ||
    dirExists(join(grokHome(resolver), "logs"))
  ) {
    supported.push("grok");
  }
  if (dirExists(augmentSessionsRoot(resolver))) supported.push("augment");
  if (codebuffRoots(resolver).some((root) => dirExists(root))) supported.push("codebuff");
  if (codebuddyRoots(resolver).some((root) => dirExists(join(root, "projects")))) {
    supported.push("codebuddy");
  }
  if (dirExists(jcodeSessionsRoot(resolver))) supported.push("jcode");
  if (dirExists(muxSessionsRoot(resolver))) supported.push("mux");
  if (zedDbPaths(resolver).some((path) => existsSync(path))) supported.push("zed");
  if (cherrystudioDbPaths(resolver).some((path) => existsSync(path))) {
    supported.push("cherrystudio");
  }
  if (
    existsSync(zcodeDbPath(zcodeRoot(resolver))) ||
    hasFileWithSuffix(zcodeProjectsRoot(zcodeRoot(resolver)), ".jsonl")
  ) {
    supported.push("zcode");
  }
  if (
    existsSync(devinDbPath(resolver)) ||
    hasFileWithSuffix(devinDesktopRoot(resolver), ".ndjson")
  ) {
    supported.push("devin");
  }
  if (rooTaskRoots(resolver).some((root) => dirExists(root))) supported.push("roo");
  if (
    clineTaskRoots(resolver).some((root) => dirExists(root)) ||
    clineCliSessionRoots(resolver).some((root) => dirExists(root))
  ) {
    supported.push("cline");
  }
  if (kilocodeTaskRoots(resolver).some((root) => dirExists(root))) supported.push("kilocode");
  if (dirExists(opencodereviewSessionsRoot(resolver))) supported.push("opencodereview");
  {
    const trajectories = traeTrajectoriesRoot(resolver);
    if (trajectories !== undefined && dirExists(trajectories)) supported.push("trae");
  }
  {
    const root = deepseekRoot(resolver);
    if (
      dirExists(root) &&
      listFiles(
        root,
        (name) =>
          name === "session.jsonl" ||
          name === "session.jsonl.zstd" ||
          name.endsWith(".db") ||
          name.endsWith(".sqlite") ||
          name.endsWith(".sqlite3"),
      ).length > 0
    ) {
      supported.push("deepseek");
    }
  }
  if (hasFileWithSuffix(reasonixSessionsGlobRoot(resolver), ".acp.json")) {
    supported.push("reasonix");
  }
  if (kiroDbPaths(resolver).some((path) => existsSync(path))) supported.push("kiro");
  if (hasImmediateChildFile(fxSessionsRoot(resolver), "usage-v2.json")) supported.push("fx");

  const home = resolver.home();
  const unsupported: string[] = [];
  if (dirExists(join(home, ".cursor")) || dirExists(join(home, "AppData", "Roaming", "Cursor"))) {
    unsupported.push("Cursor");
  }
  if (hasAiderFiles(home)) unsupported.push("aider");
  // Crush keeps its usage in a project-local .crush/crush.db under the working
  // directory, not a home path; the reader contract resolves roots only from
  // home + env, so there is no project directory to discover it from
  if (crushDetected(home, resolver)) unsupported.push("Crush");
  // GitLab Duo CLI keeps sessions server-side (GraphQL checkpoints); the only
  // local artifact is a prompt-history file with no token counts
  if (duoDetected(home, resolver)) unsupported.push("GitLab Duo CLI");
  return { supported, unsupported };
}

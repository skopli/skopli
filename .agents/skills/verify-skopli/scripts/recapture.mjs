#!/usr/bin/env node
// Recapture mode: rebuilds the harness capture matrix on this machine, serially, and reports drift
// against the golden corpus. Data lives in ../recapture/harnesses.json; this file is the only runner.
import { spawn, spawnSync } from "node:child_process";
import {
  cpSync,
  createWriteStream,
  existsSync,
  openSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { connect } from "node:net";
import { hostname, platform, userInfo } from "node:os";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { delimiter, dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "./cli.mjs";

const help = `usage: recapture.mjs <step> [options]

steps (each idempotent; "all" runs them in order and always ends with stop):
  list      print every cell with its planned outcome and exit
  env       download the pinned llama.cpp build and model (model byte-size checked), clone the
            target project, start llama-server and the usage gateway, prove the cache rises
  install   install every harness the manifest can install, write cli-versions.txt
  run       run the selected cells one at a time with an isolated home each
  report    write cell-notes.<os>.tsv, matrix.<os>.md and drift.json from the cell results
  package   tar the run directory without target copies, model or server binaries
  stop      stop the server and gateway this run started, by PID, and on Windows undo the
            user PATH of an install that was interrupted
  all       env, install, run, report, package, stop

options:
  --out <dir>        run directory (default .verify/recapture/<utc-stamp>; reuse it to resume)
  --cache <dir>      download cache shared across runs (default .verify/recapture/cache)
  --only <h[/shape]> restrict to a harness or one cell; repeatable
  --os <os>          plan for linux, darwin or windows; only list and --dry-run accept another OS
  --timeout <s>      wall-clock budget per cell across all turns, in seconds (default 600)
  --resume           skip cells in --out that already ran; rerun error, not-installed and changed skips
  --baseline <dir>   a previous run directory; report flags CLI version changes against it and
                     does not count a TODO it already reported
  --dry-run          print the resolved plan as JSON and exit
  --quiet            no progress lines on stderr
  --help             this text

exit codes: 0 ok and no drift, 1 an invariant failed, 2 usage error, 3 drift or a TODO cell needs an agent

examples:
  recapture.mjs list --only codex
  recapture.mjs all --out .verify/recapture/20261201T000000Z
  recapture.mjs run --out .verify/recapture/20261201T000000Z --only goose/openai-chat --resume`;
const STEPS = ["list", "env", "install", "run", "report", "package", "stop", "all"];
const args = parseArgs(
  help,
  ["--out", "--cache", "--only", "--os", "--timeout", "--baseline"],
  ["--resume"],
);
const [step] = args.positional;
if (!STEPS.includes(step)) args.usage(`unknown step ${step ?? "(none)"}`);

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../../..");
const manifest = JSON.parse(readFileSync(join(here, "../recapture/harnesses.json"), "utf8"));
const seeds = join(here, "../recapture/seeds");
const HOST = { linux: "linux", darwin: "darwin", win32: "windows" }[platform()];
const OS = args.one("--os", HOST);
if (!["linux", "darwin", "windows"].includes(OS)) args.usage(`unsupported os ${OS}`);
if (OS !== HOST && step !== "list" && !args.dryRun)
  args.usage(`--os ${OS} only plans; ${step} has to run on ${OS}`);
const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\.\d+/, "");
const out = resolve(args.one("--out", join(root, ".verify/recapture", stamp)));
const cache = resolve(args.one("--cache", join(root, ".verify/recapture/cache")));
const timeout = Number(args.one("--timeout", "600"));
if (!(timeout > 0)) args.usage("--timeout must be a positive number of seconds");
const only = args.many("--only");
const say = args.log;
// Harness installs land in the cache: a pnpm home for packages and a separate HOME for vendor scripts,
// so a run never touches the machine's own globals or dotfiles.
// The repo's own pnpm install keeps the caller's environment, since another PNPM_HOME means another store.
const hostEnv = { ...process.env };
process.env.PNPM_HOME = join(cache, "pnpm-home");
const scriptHome = join(cache, "script-home");
process.env.UV_TOOL_DIR = join(cache, "uv-tools");
process.env.UV_TOOL_BIN_DIR = join(cache, "uv-bin");
const basePath = process.env.PATH;
// Windows vendor installers append their bin dir to the user PATH in the registry; install() moves
// the entries an installer added into this file and restores the operator's user PATH.
const vendorFile = join(cache, "vendor-path.json");
const pathBeforeFile = join(cache, "user-path-before.json");
const vendorPath = () => {
  try {
    return OS === "windows" ? JSON.parse(readFileSync(vendorFile, "utf8")) : [];
  } catch {
    return [];
  }
};
// A ps1 installer adds its bin dir to the operator's user PATH. Only the entries under the script
// home are moved out (into vendor-path.json, put on PATH for turns), before and after each install,
// so an interrupted install is undone by the next and edits made meanwhile are kept. The kind read
// before the install is written back, in case the installer turned REG_EXPAND_SZ into REG_SZ.
// Vendor installers scan PATH (kimi's migrates kimi shims it finds there, mimo's skips the install
// when a mimo already runs), so they see only the cache dirs, System32 and Git.
const installerPath = () => {
  const sys = join(process.env.SystemRoot ?? process.env.windir ?? "", "System32");
  const root = resolve(cache).toLowerCase() + sep;
  return [
    ...process.env.PATH.split(delimiter).filter((d) => resolve(d).toLowerCase().startsWith(root)),
    sys,
    join(sys, "Wbem"),
    join(sys, "WindowsPowerShell", "v1.0"),
    join(sys, "OpenSSH"),
    ...(BASH === "bash" ? [] : [dirname(BASH)]),
  ].join(delimiter);
};
// Installers rewrite the whole value read through .NET, which expands %VAR% with the script-home
// USERPROFILE, so after an install each entry equal to an expansion of a pre-install entry gets its
// raw spelling back, and only new entries under the script home count as the installer's.
const moveVendorPath = (id, before, ienv = {}, post = false) => {
  const cur = readUserPath();
  if (!cur) {
    console.error(`install ${id}: could not read the user PATH; check it for script-home entries`);
    return null;
  }
  const lower = Object.fromEntries(Object.entries(ienv).map(([k, v]) => [k.toLowerCase(), v]));
  const expand = (e, vars) => e.replace(/%([^%;]+)%/g, (m, n) => vars[n.toLowerCase()] ?? m);
  const real = Object.fromEntries(
    Object.entries(process.env).map(([k, v]) => [k.toLowerCase(), v]),
  );
  // Installers may trim and unquote entries when they rewrite the value.
  const norm = (e) => e.trim().replace(/^"|"$/g, "").toLowerCase();
  const pre = (before?.raw ?? "").split(";").filter((e) => e.trim());
  // An entry's own spelling wins over another entry's expansion to the same directory.
  const spelling = new Map([
    ...pre.flatMap((e) => [expand(e, lower), expand(e, real)].map((x) => [norm(x), e])),
    ...pre.map((e) => [norm(e), e]),
  ]);
  const root = resolve(scriptHome).toLowerCase() + sep;
  // Empty segments (the stock value ends in ";") are kept, so an untouched value is not rewritten.
  const segs = (cur.raw ?? "").split(";");
  const mine = segs.filter(
    (d) => d.trim() && !spelling.has(norm(d)) && resolve(norm(expand(d, lower))).startsWith(root),
  );
  const keep = segs
    .filter((d) => !mine.includes(d))
    .map((d) => (d.trim() ? (spelling.get(norm(d)) ?? d) : d));
  const added = post ? keep.filter((d) => d.trim() && !spelling.has(norm(d))) : [];
  if (added.length)
    console.error(
      `install ${id}: left new user PATH entries outside the script home: ${added.join(";")}`,
    );
  const kind = before?.kind ?? cur.kind;
  const raw =
    keep.some((d) => d.trim()) || (before ? before.raw !== null : cur.raw !== null)
      ? keep.join(";")
      : null;
  if (mine.length) {
    writeFileSync(
      `${vendorFile}.tmp`,
      JSON.stringify([
        ...new Set([
          ...vendorPath(),
          ...mine.map((d) => expand(d, lower).trim().replace(/^"|"$/g, "")),
        ]),
      ]),
    );
    renameSync(`${vendorFile}.tmp`, vendorFile);
  }
  const changed = mine.length || kind !== cur.kind || (before && raw !== cur.raw);
  if (changed && !restoreUserPath({ raw, kind })) {
    console.error(
      `install ${id}: could not restore the user PATH (script-home entries: ${mine.join(";")})`,
    );
    return null;
  }
  return { raw: changed ? raw : cur.raw, kind };
};
// The raw value and its registry kind are written as given, so %VAR% entries stay unexpanded;
// null means the read failed, and then nothing is moved or restored.
const readUserPath = () => {
  const r = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      "$ErrorActionPreference = 'Stop'; [Console]::OutputEncoding = [Text.Encoding]::UTF8; $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment'); $raw = $k.GetValue('Path', $null, 'DoNotExpandEnvironmentNames'); " +
        "ConvertTo-Json -Compress @{ raw = $raw; kind = $(if ($raw -ne $null) { [string]$k.GetValueKind('Path') } else { 'ExpandString' }); " +
        "expanded = [Environment]::GetEnvironmentVariable('Path', 'User') }",
    ],
    { encoding: "utf8" },
  );
  try {
    const v = r.status === 0 ? JSON.parse(r.stdout) : null;
    return v && !`${v.raw}${v.expanded}`.includes("\uFFFD") ? v : null;
  } catch {
    return null;
  }
};
const restoreUserPath = ({ raw, kind }) =>
  spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      "$ErrorActionPreference = 'Stop'; $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true); " +
        "if ($env:RECAPTURE_USER_PATH_SET -eq '1') { $k.SetValue('Path', [string]$env:RECAPTURE_USER_PATH, [Microsoft.Win32.RegistryValueKind]$env:RECAPTURE_USER_PATH_KIND) } " +
        "else { $k.DeleteValue('Path', $false) }; " +
        // Setting and clearing a variable through .NET broadcasts WM_SETTINGCHANGE to Explorer.
        "[Environment]::SetEnvironmentVariable('RECAPTURE_PATH_SYNC', '1', 'User'); " +
        "[Environment]::SetEnvironmentVariable('RECAPTURE_PATH_SYNC', $null, 'User')",
    ],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        RECAPTURE_USER_PATH: raw ?? "",
        RECAPTURE_USER_PATH_SET: raw === null || raw === undefined ? "0" : "1",
        RECAPTURE_USER_PATH_KIND: kind,
      },
    },
  ).status === 0;
const toolPath = () => {
  const dots = existsSync(scriptHome)
    ? readdirSync(scriptHome).map((d) => join(scriptHome, d, "bin"))
    : [];
  process.env.PATH = [
    process.env.PNPM_HOME,
    // Newer pnpm links global bins into PNPM_HOME/bin and refuses `add -g` unless it is on PATH.
    join(process.env.PNPM_HOME, "bin"),
    ...[
      process.env.UV_TOOL_BIN_DIR,
      join(scriptHome, ".local/bin"),
      join(scriptHome, "bin"),
      ...dots,
      ...vendorPath(),
    ].filter((d) => OS !== "windows" || existsSync(d)),
    basePath,
  ].join(delimiter);
};
toolPath();
const LLAMA = {
  build: "b11720",
  port: 8090,
  gateway: 8091,
  model: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
  bytes: 1117320736,
  modelUrl:
    "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf",
  asset: { linux: "ubuntu-x64.tar.gz", darwin: "macos-arm64.tar.gz", windows: "win-cpu-x64.zip" },
};
const origin = `http://127.0.0.1:${LLAMA.gateway}`;
const KEY = "sk-local";
// Parent variables that would point a harness at a real home, account or provider are never passed to a cell.
const LEAKY =
  /^(XDG_\w+|AWS_\w+|\w+_API_KEY|\w+_API_BASE|\w+_BASE_URL|\w+_TOKEN|GOOGLE_APPLICATION_CREDENTIALS|CLAUDE_CONFIG_DIR|CODEX_HOME|DSH_HOME|HERMES_\w+|JCODE_HOME|KIMI_\w+|GOOSE_\w+|PRIME_AGENT_\w+|GROK_\w+|CURSOR_CONFIG_DIR|MIMOCODE_\w+|MIMO_\w+|UV_INSTALL_DIR|UV_UNMANAGED_INSTALL)$/;
const cleanEnv = () =>
  Object.fromEntries(Object.entries(process.env).filter(([k]) => !LEAKY.test(k)));

const files = (dir, base = dir) =>
  existsSync(dir)
    ? readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
        e.isSymbolicLink()
          ? []
          : e.isDirectory()
            ? files(join(dir, e.name), base)
            : [relative(base, join(dir, e.name)).replaceAll("\\", "/")],
      )
    : [];
// Seeds are recorded per OS. Linux and macOS share a HOME-relative layout, so either may stand in
// for the other unless the seed writes under Library/ or AppData/.
const seedDir = (name) =>
  [OS, ...({ linux: ["darwin"], darwin: ["linux"] }[OS] ?? [])]
    .map((o) => [o, join(seeds, o, name)])
    .find(
      ([o, d]) =>
        existsSync(d) && (o === OS || !files(d).some((f) => /^(Library|AppData)\//.test(f))),
    )?.[1];
// A cell is TODO when a field still names a gap the recovered kit could not source on this OS.
// An install channel can differ per OS (a PowerShell installer on Windows); installOs overrides install.
const installOf = (h) => h.installOs?.[OS] ?? h.install;
const todoOf = (h, c) =>
  c.first === "TODO" || c.next === "TODO"
    ? "command not recorded"
    : !installOf(h) || installOf(h).todo
      ? `install: ${installOf(h)?.todo ?? "channel not recorded"}`
      : OS === "windows" && installOf(h).script
        ? "install: no Windows channel for the vendor script"
        : c.seed && !seedDir(c.seed) && !c.config?.kind
          ? `no ${OS} config seed`
          : null;
const storeOf = (h) => h.storeOs?.[OS] ?? h.store ?? [];
const cells = Object.entries(manifest.harnesses)
  .flatMap(([harness, h]) =>
    Object.entries(h.shapes).map(([shape, c]) => {
      const skip = c.skip ?? c.skipOs?.[OS];
      const todo = skip ? null : todoOf(h, c);
      return {
        harness,
        shape,
        id: `${harness}/${shape}`,
        h,
        c,
        skip,
        todo,
        plan: skip ? `skip:${skip}` : todo ? "todo" : "run",
      };
    }),
  )
  .filter((x) => !only.length || only.some((o) => o === x.harness || o === x.id));
const unmatched = only.filter((o) => !cells.some((x) => o === x.harness || o === x.id));
if (unmatched.length) args.usage(`--only matched no cell: ${unmatched.join(" ")}`);

// cmd.exe gets one command line with every argument double-quoted, because Node only joins
// arguments when it spawns through a shell.
// The command itself stays bare when it can: cmd.exe resolves %~dp0 of a quoted .cmd shim found on
// PATH against the working directory, so a quoted "pnpm" runs corepack from the wrong folder.
const winLine = (argv) =>
  argv.map((a, i) => (!i && /^[\w.-]+$/.test(a) ? a : `"${a.replaceAll('"', '""')}"`)).join(" ");
const state = join(out, "state.json");
// A file cut short by a kill mid-write reads as missing.
const readJson = (f, fallback) => {
  try {
    return JSON.parse(readFileSync(f, "utf8"));
  } catch {
    return fallback;
  }
};
// Git Bash puts its GNU tar first on PATH, which reads a drive-letter path as a remote host and cannot open a zip.
const TAR =
  OS === "windows"
    ? join(process.env.SystemRoot ?? process.env.windir ?? "", "System32", "tar.exe")
    : "tar";
const saveState = (patch) =>
  writeFileSync(state, JSON.stringify({ ...readJson(state, {}), ...patch }, null, 2));
const sh = (cmd, a, o = {}) =>
  OS === "windows" && !o.noShell
    ? spawnSync(winLine([cmd, ...a]), { encoding: "utf8", maxBuffer: 64 << 20, shell: true, ...o })
    : spawnSync(cmd, a, { encoding: "utf8", maxBuffer: 64 << 20, ...o });
const killTree = (pid) => {
  if (!pid) return;
  if (OS === "windows") sh("taskkill", ["/PID", String(pid), "/T", "/F"], { noShell: true });
  else {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {}
  }
};
// The OS start time tells a process we spawned apart from a later one that reused its PID.
const started = (pid) =>
  (OS === "windows"
    ? sh(
        "powershell",
        ["-NoProfile", "-Command", `(Get-Process -Id ${pid}).StartTime.ToFileTimeUtc()`],
        { noShell: true },
      )
    : sh("ps", ["-o", "lstart=", "-p", String(pid)], { noShell: true })
  ).stdout?.trim() || null;
const ours = (s, k) =>
  !!s[`${k}Pid`] && !!s[`${k}Start`] && started(s[`${k}Pid`]) === s[`${k}Start`];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// fail throws so the top level can still stop what an `all` run started before exiting 1.
const fail = (message) => {
  throw new Error(message);
};

// Downloads land beside the target as .part and are renamed only when complete and the expected size.
const download = async (url, file, bytes) => {
  const res = await fetch(url, { headers: { "user-agent": "skopli-recapture" } });
  if (!res.ok) fail(`download ${url} answered ${res.status}`);
  mkdirSync(dirname(file), { recursive: true });
  await pipeline(Readable.fromWeb(res.body), createWriteStream(`${file}.part`));
  const got = statSync(`${file}.part`).size;
  if (bytes && got !== bytes) fail(`${url} gave ${got} B, expected ${bytes} B`);
  renameSync(`${file}.part`, file);
};
const portBusy = (port) =>
  new Promise((r) => {
    const c = connect(port, "127.0.0.1");
    c.once("connect", () => (c.destroy(), r(true)));
    c.once("error", () => r(false));
  });
const findFile = (dir, name) => {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e.name === name) return p;
  }
  return null;
};
const post = async (path, body, headers = {}) => {
  const res = await fetch(origin + path, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${KEY}`,
      "x-api-key": KEY,
      "anthropic-version": "2023-06-01",
      ...headers,
    },
    body: JSON.stringify(body),
  });
  return res.json();
};
// Ready means the process we started is still alive and the URL answers.
const waitFor = async (url, seconds, child) => {
  for (let i = 0; i < seconds; i++) {
    if (child.exitCode !== null || child.signalCode !== null || child.spawnError) return false;
    try {
      if ((await fetch(url)).ok) return true;
    } catch {}
    await sleep(1000);
  }
  return false;
};

async function env() {
  // home.mjs and reconcile.mjs read through the built facade and native addon.
  // Without a TTY pnpm aborts instead of recreating node_modules; CI=true lets the install proceed.
  for (const [a, env] of [
    [["install", "--frozen-lockfile"], { ...hostEnv, CI: "true" }],
    [["run", "build"], hostEnv],
  ])
    if (sh("pnpm", a, { cwd: root, stdio: "inherit", env }).status !== 0)
      fail(`pnpm ${a.join(" ")} failed`);
  const bin = join(cache, `llama-${LLAMA.build}-${OS}`);
  let server =
    existsSync(bin) && findFile(bin, OS === "windows" ? "llama-server.exe" : "llama-server");
  if (!server) {
    // The pinned build fixes the asset URL; the GitHub API rate-limits anonymous callers.
    // download() keeps the archive as .part until the transfer completes.
    const asset = { name: `llama-${LLAMA.build}-bin-${LLAMA.asset[OS]}` };
    const archive = join(cache, asset.name);
    if (!existsSync(archive)) {
      say(`download ${asset.name}`);
      await download(
        `https://github.com/ggml-org/llama.cpp/releases/download/${LLAMA.build}/${asset.name}`,
        archive,
      );
    }
    mkdirSync(bin, { recursive: true });
    const x = sh(TAR, ["xf", archive, "-C", bin]);
    if (x.status !== 0) {
      // Without the API there is no size to check, so a bad archive and its partial extraction are
      // dropped for the next env run.
      rmSync(archive, { force: true });
      rmSync(bin, { recursive: true, force: true });
      fail(`extract ${asset.name}: ${x.stderr}`);
    }
    server = findFile(bin, OS === "windows" ? "llama-server.exe" : "llama-server");
    if (!server) fail(`llama-server not found in ${asset.name}`);
  }
  const model = join(cache, LLAMA.model);
  if (!existsSync(model) || statSync(model).size !== LLAMA.bytes) {
    say(`download ${LLAMA.model}`);
    await download(LLAMA.modelUrl, model, LLAMA.bytes);
  }
  const target = join(cache, "target/p-limit");
  if (
    !existsSync(target) &&
    sh("git", ["clone", "--depth", "1", "https://github.com/sindresorhus/p-limit.git", target])
      .status !== 0
  )
    fail("clone p-limit failed");
  mkdirSync(out, { recursive: true });
  const prior = readJson(state, {});
  if (prior.serverPid || prior.gatewayPid)
    fail(`server or gateway already recorded in ${state}; run stop first`);
  for (const port of [LLAMA.port, LLAMA.gateway])
    if (await portBusy(port)) fail(`port ${port} is already in use; stop whatever holds it`);
  const flags = [
    "-m",
    model,
    "--host",
    "127.0.0.1",
    "--port",
    String(LLAMA.port),
    "-c",
    "131072",
    "-np",
    "2",
    "--rope-scaling",
    "yarn",
    "--rope-scale",
    "2",
    "--yarn-orig-ctx",
    "32768",
    "--alias",
    "qwen",
    ...(OS === "windows" ? ["-n", "1024"] : []),
    // Recapture runs macOS in a VM, whose paravirtual Metal generates at about 1 token/s against
    // about 70 on the CPU, so darwin always infers on the CPU.
    ...(OS === "darwin" ? ["-ngl", "0"] : []),
  ];
  const slog = openSync(join(out, "llama-server.txt"), "w");
  const srv = spawn(server, flags, {
    detached: true,
    windowsHide: true,
    stdio: ["ignore", slog, slog],
  });
  srv.on("error", (e) => (srv.spawnError = e));
  srv.unref();
  saveState({
    serverPid: srv.pid,
    serverStart: srv.pid && started(srv.pid),
    os: OS,
    target,
    llama: { ...LLAMA, asset: undefined, flags: flags.slice(2) },
  });
  if (!(await waitFor(`http://127.0.0.1:${LLAMA.port}/health`, 300, srv)))
    fail("llama-server exited or was not healthy after 300 s, see llama-server.txt");
  const gw = spawn(
    process.execPath,
    [
      join(here, "gateway.ts"),
      "--port",
      String(LLAMA.gateway),
      "--upstream",
      `http://127.0.0.1:${LLAMA.port}`,
      "--log",
      join(out, "gateway-usage.jsonl"),
      "--quiet",
    ],
    {
      detached: true,
      windowsHide: true,
      stdio: ["ignore", "ignore", openSync(join(out, "gateway-stderr.txt"), "w")],
    },
  );
  gw.on("error", (e) => (gw.spawnError = e));
  gw.unref();
  saveState({ gatewayPid: gw.pid, gatewayStart: gw.pid && started(gw.pid) });
  if (!(await waitFor(`${origin}/v1/models`, 60, gw)))
    fail("gateway exited or was not answering after 60 s, see gateway-stderr.txt");
  // Two turns with a shared prefix: turn two must report more cached prompt tokens than turn one.
  const system = `You answer questions about the p-limit package. ${"Keep answers short and factual. ".repeat(40)}`;
  const t1 = [
    { role: "system", content: system },
    { role: "user", content: "What does p-limit do?" },
  ];
  const c1 = await post("/v1/chat/completions", { model: "qwen", messages: t1, max_tokens: 32 });
  const c2 = await post("/v1/chat/completions", {
    model: "qwen",
    max_tokens: 32,
    messages: [
      ...t1,
      c1.choices[0].message,
      { role: "user", content: "And what is its default export?" },
    ],
  });
  const m = (msgs) =>
    post("/v1/messages", {
      model: "qwen",
      max_tokens: 32,
      system: [{ type: "text", text: system, cache_control: { type: "ephemeral" } }],
      messages: msgs,
    });
  const a1 = await m([{ role: "user", content: "What does p-limit do?" }]);
  const a2 = await m([
    { role: "user", content: "What does p-limit do?" },
    { role: "assistant", content: a1.content },
    { role: "user", content: "And what is its default export?" },
  ]);
  const proof = {
    chat: [c1.usage, c2.usage],
    messages: [a1.usage, a2.usage],
    chatRises:
      (c2.usage?.prompt_tokens_details?.cached_tokens ?? 0) >
      (c1.usage?.prompt_tokens_details?.cached_tokens ?? 0),
    messagesRises:
      (a2.usage?.cache_read_input_tokens ?? 0) > (a1.usage?.cache_read_input_tokens ?? 0),
  };
  writeFileSync(join(out, "cache-proof.json"), JSON.stringify(proof, null, 2));
  if (!proof.chatRises || !proof.messagesRises)
    fail(`cache counters did not rise on turn two, see ${join(out, "cache-proof.json")}`);
  say(`env: ok, server ${srv.pid}, gateway ${gw.pid}, cache proof passed`);
}

// Undo an install that never reached its post-install restore, expanding with the env it ran under.
const recoverUserPath = () => {
  const saved = OS === "windows" && readJson(pathBeforeFile, null);
  if (saved && moveVendorPath(`${saved.id} (interrupted)`, saved, saved.vars ?? {}, true))
    rmSync(pathBeforeFile, { force: true });
};
// A recorded PID whose start time no longer matches belongs to someone else and is left alone.
function stop() {
  const s = readJson(state, {});
  const done = ["server", "gateway"].map((k) => {
    const pid = s[`${k}Pid`];
    if (!pid) return "-";
    if (!ours(s, k)) return `${pid} (not ours, left alone)`;
    killTree(pid);
    return String(pid);
  });
  if (existsSync(state))
    saveState({ serverPid: null, serverStart: null, gatewayPid: null, gatewayStart: null });
  recoverUserPath();
  say(`stop: ${done.join(" ")}`);
}

const which = (bin) =>
  sh(OS === "windows" ? "where" : "which", [bin], { noShell: true })
    .stdout?.split(/\r?\n/)[0]
    ?.trim() || null;
const binOf = (c) => (Array.isArray(c.first) ? c.first[0] : null);
function install() {
  mkdirSync(out, { recursive: true });
  const harnesses = [...new Set(cells.filter((x) => x.plan === "run").map((x) => x.harness))];
  const lines = [];
  recoverUserPath();
  for (const id of harnesses) {
    const { shapes } = manifest.harnesses[id];
    const how = installOf(manifest.harnesses[id]);
    const bin = Object.values(shapes).map(binOf).find(Boolean);
    let r = null;
    mkdirSync(scriptHome, { recursive: true });
    const env = { ...cleanEnv(), ...homeEnv(scriptHome) };
    const agents = join(scriptHome, "Library/LaunchAgents");
    const plists = () =>
      existsSync(agents)
        ? readdirSync(agents)
            .filter((f) => f.endsWith(".plist"))
            .map((f) => `${f}:${statSync(join(agents, f)).mtimeMs}`)
        : [];
    const before = new Set(plists());
    // pnpm 10+ skips build scripts unless allowed; claude, opencode and kimchi fetch their binary in one.
    if (how?.pnpm) r = sh("pnpm", ["add", "-g", `--allow-build=${how.pnpm}`, how.pnpm], { env });
    else if (how?.uv)
      r = sh(
        "uv",
        [
          "tool",
          "install",
          "--python",
          how.python,
          ...(how.with ?? []).flatMap((w) => ["--with", w]),
          how.uv,
        ],
        { env },
      );
    else if (how?.ps1 && OS === "windows") {
      const temp = join(scriptHome, "AppData", "Local", "Temp");
      mkdirSync(temp, { recursive: true });
      const ienv = {
        // mimocode's installer reads a generic VERSION as a version pin.
        ...Object.fromEntries(
          Object.entries(env).filter(([k]) => !["PATH", "VERSION"].includes(k.toUpperCase())),
        ),
        TEMP: temp,
        TMP: temp,
        ...Object.fromEntries(
          Object.entries(how.env ?? {}).map(([k, v]) => [
            k,
            fill(v, { home: scriptHome.replaceAll("\\", "/") }),
          ]),
        ),
        RECAPTURE_PS1: how.ps1,
        PATH: installerPath(),
      };
      const pathBefore = !existsSync(pathBeforeFile) && moveVendorPath(id, null);
      if (!pathBefore) {
        console.error(
          `install ${id}: skipped, the user PATH could not be read or restored (pending ${pathBeforeFile})`,
        );
      } else {
        const vars = ["USERPROFILE", "APPDATA", "LOCALAPPDATA", "HOME", "TEMP", "TMP"];
        writeFileSync(
          `${pathBeforeFile}.tmp`,
          JSON.stringify({
            ...pathBefore,
            id,
            vars: Object.fromEntries(vars.map((k) => [k, ienv[k]])),
          }),
        );
        renameSync(`${pathBeforeFile}.tmp`, pathBeforeFile);
        const t0 = new Date();
        r = sh(
          "powershell",
          [
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            `[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; $ProgressPreference = "SilentlyContinue"; & ([scriptblock]::Create((irm $env:RECAPTURE_PS1))) ${(how.args ?? []).map((a) => (/^-\w+$/.test(a) ? a : `'${a.replaceAll("'", "''")}'`)).join(" ")}`,
          ],
          {
            noShell: true,
            timeout: 900000,
            cwd: scriptHome,
            env: ienv,
          },
        );
        reapSince(t0, [cache]);
        if (moveVendorPath(id, pathBefore, ienv, true)) rmSync(pathBeforeFile, { force: true });
      }
    } else if (how?.script && OS !== "windows")
      r = sh(
        "bash",
        ["-c", `curl -fsSL ${how.script} | bash -s -- ${(how.args ?? []).join(" ")}`],
        {
          env: { ...env, ...how.env },
        },
      );
    // launchd is per user, not per HOME: jcode's installer bootstraps a hotkey LaunchAgent from the
    // script home into the real login session. Unload only a job this install wrote and launchd
    // loaded from that plist, so an operator's own agent under the same label stays.
    if (OS === "darwin")
      for (const f of plists().filter((p) => !before.has(p))) {
        const plist = join(agents, f.slice(0, f.lastIndexOf(":")));
        const label = sh("plutil", ["-extract", "Label", "raw", plist]).stdout.trim();
        const job = label && sh("launchctl", ["print", `gui/${userInfo().uid}/${label}`]);
        if (job?.status === 0 && job.stdout.includes(`path = ${plist}\n`))
          sh("launchctl", ["bootout", `gui/${userInfo().uid}/${label}`]);
      }
    toolPath();
    if (r && r.status !== 0) say(`install ${id}: exit ${r.status}`);
    const path = bin && which(bin);
    const v = path ? versionOf(bin, manifest.harnesses[id]) : null;
    lines.push(
      [
        id,
        path ? path.replace(userInfo().homedir, "~") : "MISSING",
        v ? (v.stdout || v.stderr || v.error?.code || "").trim().split(/\r?\n/)[0] : "-",
      ].join("\t"),
    );
  }
  writeFileSync(join(out, "cli-versions.txt"), lines.join("\n") + "\n");
  say(`install: ${lines.filter((l) => !l.includes("MISSING")).length}/${lines.length} on PATH`);
}

const fill = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
const CONFIG = {
  opencode: (shape) => ({
    $schema: "https://opencode.ai/config.json",
    model: "llama/qwen",
    permission: { "*": "allow" },
    provider: {
      llama: {
        npm: {
          "openai-chat": "@ai-sdk/openai-compatible",
          "openai-responses": "@ai-sdk/openai",
          "anthropic-messages": "@ai-sdk/anthropic",
        }[shape],
        name: "llama.cpp",
        options: { baseURL: `${origin}/v1`, apiKey: KEY },
        models: { qwen: { name: "qwen", limit: { context: 32768, output: 1024 } } },
      },
    },
  }),
  pi: (shape) => ({
    providers: {
      llama: {
        baseUrl: `${origin}/v1`,
        apiKey: KEY,
        api: {
          "openai-chat": "openai-completions",
          "openai-responses": "openai-responses",
          "anthropic-messages": "anthropic-messages",
        }[shape],
        models: [
          {
            id: "qwen",
            name: "qwen",
            reasoning: false,
            input: ["text"],
            cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
            contextWindow: 32768,
            maxTokens: 1024,
          },
        ],
      },
    },
  }),
};
// On Windows the helper needs Git Bash; a bare "bash" can resolve to the WSL launcher in System32,
// which would report Linux paths. Git for Windows' bash.exe is found above git's own directories.
const BASH =
  HOST === "windows"
    ? ([
        spawnSync("git", ["--exec-path"], { encoding: "utf8" }).stdout,
        ...(spawnSync("where", ["git"], { encoding: "utf8" }).stdout?.split(/\r?\n/) ?? []),
      ]
        .map((p) => p?.trim())
        .filter(Boolean)
        .flatMap((p) => [1, 2, 3].map((n) => resolve(p, ...Array(n).fill(".."), "bin", "bash.exe")))
        .find((b) => existsSync(b)) ?? "bash")
    : "bash";
let bashChecked = OS !== "windows";
const homeEnv = (home) => {
  if (
    !bashChecked &&
    !/MINGW|MSYS|CYGWIN/.test(sh(BASH, ["-c", "uname -s"], { noShell: true }).stdout ?? "")
  )
    fail(`${BASH} is not Git Bash; install Git for Windows`);
  bashChecked = true;
  const r = sh(BASH, [join(here, "os-home.sh"), "--plain", home], { noShell: true });
  const env = Object.fromEntries(
    (r.stdout ?? "")
      .trim()
      .split("\n")
      .map((l) => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1)]),
  );
  if (r.status !== 0 || !env.HOME) fail(`os-home.sh failed for ${home}: ${r.stderr?.trim()}`);
  return env;
};

// A turn ends when its leader exits; on Linux and macOS whatever it left in its process group is
// killed then, so no stray outlives the turn or keeps its output pipes open.
// A native .exe is spawned directly: under cmd.exe a CLI that needs CREATE_BREAKAWAY_FROM_JOB
// (hermes starts its gateway daemon that way) gets access denied.
// `where` lists a directory's matches by name, so the one cmd.exe would run is picked by PATHEXT.
const winExe = (bin, envv, cwd) => {
  const hits = (spawnSync("where", [bin], { encoding: "utf8", env: envv, cwd }).stdout ?? "")
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
  const exts = (process.env.PATHEXT ?? ".COM;.EXE;.BAT;.CMD").toLowerCase().split(";");
  const rank = (l) => {
    const i = exts.findIndex((e) => e && l.toLowerCase().endsWith(e));
    return i < 0 ? exts.length : i;
  };
  // An extensionless POSIX shim is never what cmd.exe runs.
  const runnable = hits.filter((l) => rank(l) < exts.length);
  if (!runnable.length) return undefined;
  const local = runnable.filter(
    (l) => dirname(l).toLowerCase() === dirname(runnable[0]).toLowerCase(),
  );
  const pick = local.reduce((a, l) => (rank(l) < rank(a) ? l : a));
  return /\.exe$/i.test(pick) ? pick : undefined;
};
// Windows has no process group to kill, so after a turn the processes started since its start from
// an executable under the cache or run directory are killed by PID (hermes leaves a gateway daemon).
const reapSince = (t0, roots) => {
  const r = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      "$since = [datetime]::Parse($env:RECAPTURE_SINCE); $roots = $env:RECAPTURE_ROOTS -split ';'; " +
        "Get-CimInstance Win32_Process | Where-Object { $e = $_.ExecutablePath; $_.CreationDate -ge $since -and $e -and " +
        "($roots | Where-Object { $_ -and $e.StartsWith($_, 'OrdinalIgnoreCase') }) } | ForEach-Object { $_.ProcessId }",
    ],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        RECAPTURE_SINCE: new Date(t0).toISOString(),
        RECAPTURE_ROOTS: roots.map((d) => resolve(d) + "\\").join(";"),
      },
    },
  );
  for (const pid of (r.stdout ?? "").split(/\s+/).filter(Boolean)) killTree(Number(pid));
};
let turnPid = null;
const runTurn = (argv, envv, cwd, log, limit) =>
  new Promise((done) => {
    const t0 = Date.now();
    const exe = OS === "windows" && winExe(argv[0], envv, cwd);
    const child =
      OS === "windows"
        ? exe
          ? spawn(exe, argv.slice(1), {
              cwd,
              env: envv,
              stdio: ["ignore", "pipe", "pipe"],
            })
          : spawn(winLine(argv), { cwd, env: envv, stdio: ["ignore", "pipe", "pipe"], shell: true })
        : spawn(argv[0], argv.slice(1), {
            cwd,
            env: envv,
            detached: true,
            stdio: ["ignore", "pipe", "pipe"],
          });
    turnPid = child.pid;
    let text = "";
    let timedOut = false;
    let ended = false;
    child.stdout.on("data", (d) => (text += d));
    child.stderr.on("data", (d) => (text += d));
    const timer = setTimeout(() => {
      timedOut = true;
      killTree(child.pid);
    }, limit * 1000);
    const finish = (code) => {
      if (ended) return;
      ended = true;
      turnPid = null;
      clearTimeout(timer);
      writeFileSync(
        log,
        `cmd: ${argv.join(" ")}\n${text}\nexit: ${code}${timedOut ? " (timeout)" : ""}\n`,
      );
      done({ code, timedOut, text, seconds: Math.round((Date.now() - t0) / 1000) });
    };
    child.on("error", () => finish(127));
    // Windows cannot walk the tree of an exited leader: a timeout kills the tree, and after an exit
    // reapSince kills what the turn started from the cache or run directory.
    child.on("exit", (code) => {
      if (OS !== "windows") killTree(child.pid);
      else reapSince(t0, [cache, out]);
      setTimeout(() => {
        child.stdout.destroy();
        child.stderr.destroy();
        finish(code ?? 137);
      }, 5000).unref();
    });
    child.on("close", (code) => finish(code ?? 137));
  });

// Every spelling a path takes in a store: native, forward-slash, MSYS (/c/...) and JSON-escaped.
const spellings = (p) => {
  const fwd = p.replaceAll("\\", "/");
  const msys = fwd.replace(/^([A-Za-z]):/, (_, d) => `/${d.toLowerCase()}`);
  const all = [p, fwd, msys, p.replaceAll("\\", "\\\\")];
  return [...new Set([...all, ...all.map((x) => x.replace(/^[A-Z]:/, (d) => d.toLowerCase()))])];
};
const scan = (home, cwd, paths) => {
  const needles = [
    ["username", [userInfo().username]],
    ["hostname", [hostname()]],
    ["home-path", spellings(userInfo().homedir)],
    ["cell-home", spellings(home)],
    ["cell-cwd", [...spellings(cwd), slug(cwd)]],
    ["dummy-key", [KEY, "local-only-placeholder"]],
    ["git-proxy", ["git-manager.devin.ai/proxy"]],
  ].map(([k, v]) => [k, v.filter((n) => n && n.length > 2)]);
  const hits = new Set();
  for (const f of paths) {
    const text = `${f}\n${readFileSync(join(home, f)).toString("latin1")}`;
    for (const [k, v] of needles) if (v.some((n) => text.includes(n))) hits.add(k);
    if (/\b[A-Z0-9]{1,6}~\d\b/.test(text)) hits.add("8.3-alias");
  }
  return [...hits].sort();
};

// A dead server or gateway would turn every later cell into a false capture-gap.
const alive = async (s) => {
  for (const k of ["server", "gateway"])
    if (!ours(s, k)) fail(`${k} process ${s[`${k}Pid`]} from the env step is gone; rerun env`);
  // On darwin the first probe after a long idle can reuse a pooled socket the server already closed.
  const answers = async (url) => {
    for (let i = 0; i < 3; i++) {
      if (i) await new Promise((r) => setTimeout(r, 1000));
      const ok = await fetch(url, { headers: { connection: "close" } }).then(
        (r) => r.ok,
        () => false,
      );
      if (ok) return true;
    }
    return false;
  };
  for (const url of [`http://127.0.0.1:${LLAMA.port}/health`, `${origin}/v1/models`])
    if (!(await answers(url))) fail(`${url} stopped answering; rerun env`);
};

const versions = new Map();
// A CLI that ignores USERPROFILE (jcode) gets its manifest *_HOME pointed into the script home.
const versionOf = (bin, h) => {
  if (versions.has(bin)) return versions.get(bin);
  const homes = Object.values(h?.shapes ?? {})
    .flatMap((s) => Object.entries(s.env ?? {}))
    .filter(([k]) => k.endsWith("_HOME"))
    .map(([k, v]) => [k, fill(v, { home: scriptHome.replaceAll("\\", "/") })]);
  const env = {
    ...cleanEnv(),
    ...homeEnv(scriptHome),
    ...Object.fromEntries(homes),
    ...(OS === "windows" ? { PYTHONUTF8: "1", PYTHONIOENCODING: "utf-8" } : {}),
    PATH: process.env.PATH,
  };
  // A native .exe is probed directly, so a timeout kills the CLI rather than only cmd.exe.
  const exe = OS === "windows" && winExe(bin, env);
  const t0 = new Date();
  const r = sh(exe || bin, ["--version"], { timeout: 25000, env, noShell: !!exe });
  if (OS === "windows") reapSince(t0, [cache]);
  // A failure is not cached, so a first launch slowed by a virus scan is probed again.
  if (r.status === 0) versions.set(bin, r);
  return r;
};

async function run() {
  const s = readJson(state, {});
  if (!s.gatewayPid) fail("no gateway recorded; run the env step first");
  for (const cell of cells) {
    const dir = join(out, "cells", cell.harness, cell.shape);
    const resultFile = join(dir, "result.json");
    // --resume keeps finished cells and unchanged skips, and retries the ones a broken environment ended.
    const prev = readJson(resultFile, {});
    const planned = cell.plan === "run" ? null : cell.todo ? "todo" : "skip";
    if (
      args.flag("--resume") &&
      prev.status &&
      (planned
        ? prev.status === planned && prev.reason === (cell.skip ?? cell.todo)
        : !["error", "not-installed", "skip", "todo"].includes(prev.status))
    )
      continue;
    // A retry never reuses a home: the earlier attempt is kept beside it as <shape>.attemptN.
    if (existsSync(dir)) {
      let n = 1;
      while (existsSync(`${dir}.attempt${n}`)) n++;
      renameSync(dir, `${dir}.attempt${n}`);
    }
    mkdirSync(dir, { recursive: true });
    const write = (r) => {
      writeFileSync(resultFile, JSON.stringify({ cell: cell.id, os: OS, ...r }, null, 2));
      say(`${cell.id}: ${r.status}${r.reason ? ` (${r.reason})` : ""}`);
    };
    if (cell.plan !== "run") {
      write({
        status: planned,
        reason: cell.skip ?? cell.todo,
        note: cell.c.note,
      });
      continue;
    }
    if (!which(binOf(cell.c))) {
      write({ status: "not-installed", reason: binOf(cell.c) });
      continue;
    }
    // A Windows shim can resolve on PATH yet fail to start (a skipped native postinstall, an
    // unsupported binary), which would otherwise read as a capture gap.
    const probe =
      OS === "windows" ? versionOf(binOf(cell.c), manifest.harnesses[cell.harness]) : { status: 0 };
    if (probe.status !== 0) {
      write({
        status: "not-installed",
        reason: `${binOf(cell.c)} --version ${probe.error?.code === "ETIMEDOUT" ? "timed out" : (probe.error?.code ?? `exit ${probe.status}`)}`,
      });
      continue;
    }
    const home = join(dir, "home");
    const cwd = join(dir, "work");
    cpSync(s.target, cwd, { recursive: true });
    const vars = {
      home: home.replaceAll("\\", "/"),
      cwd: cwd.replaceAll("\\", "/"),
      origin,
      session: "",
    };
    const envv = {
      ...Object.fromEntries(Object.entries(process.env).filter(([k]) => !LEAKY.test(k))),
      ...homeEnv(home),
      // Python CLIs on Windows write to a pipe in the ANSI code page and crash on non-Latin-1 output.
      ...(OS === "windows" ? { PYTHONUTF8: "1", PYTHONIOENCODING: "utf-8" } : {}),
      ...Object.fromEntries(Object.entries(cell.c.env ?? {}).map(([k, v]) => [k, fill(v, vars)])),
    };
    const seed = cell.c.seed && seedDir(cell.c.seed);
    if (seed)
      for (const f of files(seed)) {
        const t = join(home, f);
        mkdirSync(dirname(t), { recursive: true });
        writeFileSync(t, fill(readFileSync(join(seed, f), "utf8"), vars));
      }
    else if (cell.c.config?.kind) {
      const f = join(home, cell.c.config.path);
      mkdirSync(dirname(f), { recursive: true });
      writeFileSync(f, JSON.stringify(CONFIG[cell.c.config.kind](cell.shape), null, 2));
    }
    await alive(s);
    const gwlog = join(out, "gateway-usage.jsonl");
    const from = existsSync(gwlog) ? statSync(gwlog).size : 0;
    const deadline = Date.now() + timeout * 1000;
    const setup =
      cell.c.setup &&
      (await runTurn(
        cell.c.setup.map((a) => fill(a, vars)),
        envv,
        cwd,
        join(dir, "setup.txt"),
        timeout,
      ));
    const setupFailed = setup && (setup.timedOut || setup.code !== 0);
    // Files the runner or the setup command wrote are not evidence: only what the turns create or change counts.
    const mtime = (f) => statSync(join(home, f)).mtimeMs;
    const seeded = new Map(files(home).map((f) => [f, mtime(f)]));
    const turns = [];
    for (const [i, prompt] of manifest.prompts.entries()) {
      const left = Math.floor((deadline - Date.now()) / 1000);
      if (setupFailed || left < 20) {
        turns.push({ skipped: setupFailed ? "setup failed" : "cell budget spent" });
        continue;
      }
      const argv = (i ? cell.c.next : cell.c.first).map((a) => fill(a, { ...vars, prompt }));
      const t = await runTurn(argv, envv, cwd, join(dir, `run-${i + 1}.txt`), left);
      turns.push({ exit: t.code, timedOut: t.timedOut, seconds: t.seconds });
      if (!i && cell.h.session) vars.session = sessionId(cell.h.session, t.text, home) ?? "";
      if (t.timedOut) break;
    }
    // An aborted line is a reply no harness received, possibly from the previous cell's killed turn.
    const lines = (existsSync(gwlog) ? readFileSync(gwlog).subarray(from).toString() : "")
      .split("\n")
      .filter(Boolean)
      .map((l) => JSON.parse(l))
      .filter((l) => l.route && !l.aborted);
    writeFileSync(
      join(dir, "gateway-usage.jsonl"),
      lines.map((l) => `${JSON.stringify(l)}\n`).join(""),
    );
    const node = (script, a) =>
      sh(process.execPath, [join(here, script), ...a], { noShell: true, timeout: 120000 });
    const hr = node("home.mjs", [home, "--out", dir]);
    const summary = readJson(join(dir, "home-result.json"), null);
    const rc = node("reconcile.mjs", [
      home,
      cell.harness,
      join(dir, "gateway-usage.jsonl"),
      "--quiet",
    ]);
    writeFileSync(join(dir, "reconcile.json"), rc.stdout ?? "");
    const events = summary?.byHarness?.[cell.harness]?.events ?? 0;
    const roots = storeOf(cell.h);
    const written = files(home).filter(
      (f) => roots.some((p) => f === p || f.startsWith(`${p}/`)) && seeded.get(f) !== mtime(f),
    );
    // home.mjs exits 0 unless it crashed; reconcile.mjs exits 0 on agreement and 1 on a delta.
    const broken =
      hr.status !== 0 || !summary
        ? `home.mjs exit ${hr.status}`
        : rc.status !== 0 && rc.status !== 1
          ? `reconcile.mjs exit ${rc.status}`
          : null;
    const status = broken
      ? "error"
      : setupFailed
        ? setup.timedOut
          ? "hang"
          : "capture-gap"
        : turns.some((t) => t.timedOut) && !events
          ? "hang"
          : !lines.length
            ? "capture-gap"
            : !events
              ? written.length
                ? "reader-gap"
                : "capture-gap"
              : rc.status === 0
                ? "captured"
                : "captured-delta";
    write({
      status,
      reason:
        broken ??
        (setupFailed
          ? setup.timedOut
            ? "setup timed out"
            : `setup exit ${setup.code}`
          : status === "capture-gap" && !lines.length
            ? "no request reached the gateway"
            : undefined),
      setup: setup
        ? { exit: setup.code, timedOut: setup.timedOut, seconds: setup.seconds }
        : undefined,
      turns,
      gatewayLines: lines.length,
      events,
      sessions: summary?.byHarness?.[cell.harness]?.sessions ?? 0,
      diagnostics: summary?.diagnostics,
      detected: summary?.detected,
      homeExit: hr.status,
      reconcileExit: rc.status,
      layout: layoutOf(
        written.filter(
          (f) => !(cell.h.layoutIgnore ?? []).some((p) => f === p || f.startsWith(`${p}/`)),
        ),
        cwd,
      ),
      redaction: scan(home, cwd, written),
    });
  }
}
// A store layout is its set of path shapes. A segment that slugs the session cwd becomes <cwd>,
// whichever cwd it was (golden cases were captured in /home/user/project), and any other segment
// carrying an id or a date becomes *.
const slug = (p) => p.replace(/[^A-Za-z0-9]/g, "-").replace(/^-+/, "");
// Harnesses slug the cwd differently (droid and reasonix keep dots, commandcode lowercases and splits
// digit runs), so a store directory is matched on its letters and digits alone.
const cwdKey = (p) => p.toLowerCase().replace(/[^a-z0-9]/g, "");
// SQLite -wal, -shm and -journal sidecars are transient, so no golden case keeps them.
const layoutOf = (paths, cwd) =>
  [
    ...new Set(
      paths
        .filter((f) => !/\.db-(wal|shm|journal)$|\.sqlite3?-(wal|shm|journal)$/.test(f))
        .map((f) =>
          f
            .split("/")
            .map((seg) =>
              cwdKey(seg) === cwdKey(cwd)
                ? "<cwd>"
                : /\d/.test(seg) || /^-.*-$/.test(seg)
                  ? `*${seg.match(/(\.[a-z]+)+$/)?.[0] ?? ""}`
                  : seg,
            )
            .join("/"),
        ),
    ),
  ].sort();
const GOLDEN_CWD = "/home/user/project";
const sessionId = (how, text, home) => {
  if (how.stdout) return text.match(new RegExp(how.stdout))?.[1] ?? null;
  // The fixed directory before the first wildcard segment, so `sessions/session_*.json` scans `sessions`.
  const base = how.glob.slice(0, how.glob.indexOf("*")).replace(/\/[^/]*$/, "");
  for (const f of files(join(home, base)))
    for (const p of [f, dirname(f)]) {
      const m = `${base}/${p}`.match(new RegExp(how.pattern));
      if (m) return m[1];
    }
  return null;
};

// Drift is judged against golden/README.md: its status column and the case directories on disk.
function report() {
  const results = cells.map((c) =>
    readJson(join(out, "cells", c.harness, c.shape, "result.json"), {
      cell: c.id,
      status: c.plan === "run" ? "not-run" : c.plan.split(":")[0],
      reason: c.plan === "todo" ? c.todo : undefined,
    }),
  );
  const readme = readFileSync(join(root, "golden/README.md"), "utf8");
  const golden = new Map(
    readme
      .split("\n")
      .filter((l) => /^\| [a-z]/.test(l))
      .map((l) => l.split("|").map((x) => x.trim()))
      .filter((c) => c[1] !== "harness")
      .map((c) => [`${c[1]}/${c[2]}`, c[3]]),
  );
  const goldenLayout = (id) => {
    const d = join(root, "golden", id, "input");
    return existsSync(d) ? layoutOf(files(d), GOLDEN_CWD) : null;
  };
  const versions = (dir) =>
    Object.fromEntries(
      (existsSync(join(dir, "cli-versions.txt"))
        ? readFileSync(join(dir, "cli-versions.txt"), "utf8")
        : ""
      )
        .split("\n")
        .filter(Boolean)
        .map((l) => l.split("\t"))
        .map((c) => [c[0], c[2]]),
    );
  const drift = [];
  for (const r of results) {
    const caseId =
      OS === "linux"
        ? r.cell
        : existsSync(join(root, "golden", `${r.cell}-${OS}`))
          ? `${r.cell}-${OS}`
          : r.cell;
    const has = existsSync(join(root, "golden", caseId));
    const ok = r.status === "captured" || r.status === "captured-delta";
    if (ok && !has) drift.push({ cell: r.cell, kind: "newly-capturable", lane: "decision" });
    if (r.status === "reader-gap" && !has)
      drift.push({ cell: r.cell, kind: "unread-store", lane: "decision" });
    if (r.status === "not-run" || r.status === "not-installed")
      drift.push({
        cell: r.cell,
        kind: "invariant",
        reason: r.status === "not-run" ? "cell did not run" : r.reason,
        lane: "agent",
      });
    else if (!ok && has && !["skip", "todo"].includes(r.status))
      drift.push({
        cell: r.cell,
        kind: "regressed",
        status: r.status,
        lane: r.status === "reader-gap" ? "mechanical-candidate" : "decision",
      });
    if (r.status === "captured-delta")
      drift.push({ cell: r.cell, kind: "reconcile-delta", lane: "decision" });
    if (ok && has) {
      const g = goldenLayout(caseId);
      if (g && r.layout && r.layout.some((f) => !g.includes(f)))
        drift.push({
          cell: r.cell,
          kind: "layout-changed",
          golden: caseId,
          added: r.layout.filter((f) => !g.includes(f)),
          lane: "mechanical-candidate",
        });
    }
    if (r.status === "todo")
      drift.push({ cell: r.cell, kind: "todo", reason: r.reason, lane: "agent" });
    if (r.status === "error")
      drift.push({ cell: r.cell, kind: "invariant", reason: r.reason, lane: "agent" });
  }
  const base = args.one("--baseline");
  // A TODO the baseline already reported with the same reason is listed again but not counted.
  const knownTodo = new Set(
    (base ? readJson(join(resolve(base), "drift.json"), []) : [])
      .filter((d) => d.kind === "todo")
      .map((d) => `${d.cell}\t${d.reason}`),
  );
  for (const d of drift)
    if (d.kind === "todo" && knownTodo.has(`${d.cell}\t${d.reason}`)) d.known = true;
  if (base) {
    const a = versions(resolve(base)),
      b = versions(out);
    for (const k of Object.keys(b))
      if (a[k] && a[k] !== b[k])
        drift.push({
          harness: k,
          kind: "version-changed",
          from: a[k],
          to: b[k],
          lane: "mechanical-candidate",
        });
  }
  const note = (r) =>
    [
      r.status,
      r.reason ?? r.note,
      r.gatewayLines !== undefined &&
        `gateway ${r.gatewayLines} lines, ${r.events} events, ${r.sessions} sessions`,
      r.layout?.length && `store ${r.layout.join(" ")}`,
      r.redaction?.length && `redact ${r.redaction.join(",")}`,
    ]
      .filter(Boolean)
      .join("; ");
  writeFileSync(
    join(out, `cell-notes.${OS}.tsv`),
    results
      .map((r) => [r.cell.replace("/", "\t"), note(r).replaceAll("\t", " ")].join("\t"))
      .join("\n") + "\n",
  );
  const row = (r) =>
    `| ${r.cell.replace("/", " | ")} | ${r.status} | ${(r.detected ?? []).join(",") || "-"} | ${r.events ?? "-"} | ${r.sessions ?? "-"} | ${r.diagnostics ?? "-"} | ${r.gatewayLines ?? "-"} | ${r.reconcileExit === 0 ? "exact" : r.reconcileExit === 1 ? "delta" : "-"} | ${golden.get(r.cell) ?? "-"} | ${(r.reason ?? r.note ?? "").replaceAll("|", "/")} |`;
  writeFileSync(
    join(out, `matrix.${OS}.md`),
    `# Recapture matrix (${OS})\n\n| harness | shape | status | detected | events | sessions | diagnostics | gateway lines | reconcile | golden status | reason |\n|---|---|---|---|---|---|---|---|---|---|---|\n${results.map(row).join("\n")}\n`,
  );
  writeFileSync(join(out, "drift.json"), JSON.stringify(drift, null, 2));
  const counts = results.reduce((a, r) => ({ ...a, [r.status]: (a[r.status] ?? 0) + 1 }), {});
  const open = drift.filter((d) => !d.known);
  console.log(
    JSON.stringify(
      { out, os: OS, counts, drift: open.length, known: drift.length - open.length },
      null,
      2,
    ),
  );
  return drift.some((d) => d.kind === "invariant") ? 1 : open.length ? 3 : 0;
}

function pack() {
  const r = sh(
    TAR,
    [
      "czf",
      join(dirname(out), `${relative(dirname(out), out)}.tgz`),
      "--exclude=node_modules",
      "--exclude=*/cells/*/*/work",
      "-C",
      dirname(out),
      relative(dirname(out), out),
    ],
    { noShell: true },
  );
  if (r.status !== 0) fail(`package: ${r.stderr}`);
  say(`package: ${out}.tgz`);
}

const plan = cells.map((c) => ({
  cell: c.id,
  plan: c.plan,
  ...(c.skip ? { reason: c.skip } : {}),
  ...(c.todo ? { todo: c.todo } : {}),
}));
if (args.dryRun) {
  console.log(JSON.stringify({ step, os: OS, out, cache, timeout, cells: plan }, null, 2));
  process.exit(0);
}
if (step === "list") {
  for (const p of plan) console.log([p.cell, p.plan, p.reason ?? p.todo ?? ""].join("\t"));
  process.exit(0);
}
const steps = { env, install, run, report, package: pack, stop };
for (const sig of ["SIGINT", "SIGTERM"])
  process.on(sig, () => {
    killTree(turnPid);
    if (step === "all") stop();
    process.exit(1);
  });
let code = 0;
try {
  for (const s of step === "all" ? ["env", "install", "run", "report", "package"] : [step])
    code = (await steps[s]()) || code;
} catch (e) {
  console.error(`recapture: ${e.message}`);
  code = 1;
} finally {
  if (step === "all") stop();
}
process.exit(code);

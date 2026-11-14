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
  statSync,
  writeFileSync,
} from "node:fs";
import { hostname, platform, userInfo } from "node:os";
import { delimiter, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "./cli.mjs";

const help = `usage: recapture.mjs <step> [options]

steps (each idempotent; "all" runs them in order and always ends with stop):
  list      print every cell with its planned outcome and exit
  env       download the pinned llama.cpp build and model (byte-size checked), clone the
            target project, start llama-server and the usage gateway, prove the cache rises
  install   install every harness the manifest can install, write cli-versions.txt
  run       run the selected cells one at a time with an isolated home each
  report    write cell-notes.<os>.tsv, matrix.<os>.md and drift.json from the cell results
  package   tar the run directory without homes, model or server binaries
  stop      stop the server and gateway this run started, by PID
  all       env, install, run, report, package, stop

options:
  --out <dir>        run directory (default .verify/recapture/<utc-stamp>; reuse it to resume)
  --cache <dir>      download cache shared across runs (default .verify/recapture/cache)
  --only <h[/shape]> restrict to a harness or one cell; repeatable
  --os <os>          label outputs for linux, darwin or windows (default: this machine)
  --timeout <s>      wall-clock budget per cell across all turns, in seconds (default 600)
  --resume           skip cells that already have result.json in --out
  --baseline <dir>   a previous run directory; report flags CLI version changes against it
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
const OS = args.one("--os", { linux: "linux", darwin: "darwin", win32: "windows" }[platform()]);
if (!["linux", "darwin", "windows"].includes(OS)) args.usage(`unsupported os ${OS}`);
const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\.\d+/, "");
const out = resolve(args.one("--out", join(root, ".verify/recapture", stamp)));
const cache = resolve(args.one("--cache", join(root, ".verify/recapture/cache")));
const timeout = Number(args.one("--timeout", "600"));
if (!(timeout > 0)) args.usage("--timeout must be a positive number of seconds");
const only = args.many("--only");
const say = args.log;
// Global harness installs land in a pnpm home inside the cache, so a run never touches the machine's own globals.
process.env.PNPM_HOME = join(cache, "pnpm-home");
process.env.PATH = `${process.env.PNPM_HOME}${delimiter}${process.env.PATH}`;
const LLAMA = {
  build: "b11720",
  port: 8090,
  gateway: 8091,
  model: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
  bytes: 1117320736,
  modelUrl:
    "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf",
  asset: {
    linux: /bin-ubuntu-x64\.(zip|tar\.gz)$/,
    darwin: /bin-macos-arm64\.(zip|tar\.gz)$/,
    windows: /bin-win-cpu-x64\.zip$/,
  },
};
const origin = `http://127.0.0.1:${LLAMA.gateway}`;
const KEY = "sk-local";

const files = (dir, base = dir) =>
  existsSync(dir)
    ? readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
        e.isDirectory()
          ? files(join(dir, e.name), base)
          : [relative(base, join(dir, e.name)).replaceAll("\\", "/")],
      )
    : [];
// Seeds are recorded per OS. Linux and macOS share a HOME-relative layout, so either may stand in
// for the other unless the seed writes under Library/ or AppData/.
const seedDir = (name) =>
  [OS, ...({ linux: ["darwin"], darwin: ["linux"] }[OS] ?? [])]
    .map((o) => join(seeds, o, name))
    .find((d) => existsSync(d) && !files(d).some((f) => /^(Library|AppData)\//.test(f)));
// A cell is TODO when a field still names a gap the recovered kit could not source on this OS.
const todoOf = (h, c) =>
  c.first === "TODO" || c.next === "TODO"
    ? "command not recorded"
    : !h.install || h.install.todo
      ? `install: ${h.install?.todo ?? "channel not recorded"}`
      : c.seed && !seedDir(c.seed) && !c.config?.kind
        ? `no ${OS} config seed`
        : null;
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
if (only.length && !cells.length) args.usage(`--only matched no cell: ${only.join(" ")}`);

const state = join(out, "state.json");
const readJson = (f, fallback) => (existsSync(f) ? JSON.parse(readFileSync(f, "utf8")) : fallback);
const saveState = (patch) =>
  writeFileSync(state, JSON.stringify({ ...readJson(state, {}), ...patch }, null, 2));
const sh = (cmd, a, o = {}) =>
  spawnSync(cmd, a, { encoding: "utf8", shell: OS === "windows" && !o.noShell, ...o });
const killTree = (pid) => {
  if (!pid) return;
  if (OS === "windows") sh("taskkill", ["/PID", String(pid), "/T", "/F"], { noShell: true });
  else {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {
      try {
        process.kill(pid, "SIGKILL");
      } catch {}
    }
  }
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fail = (message) => {
  console.error(`recapture: ${message}`);
  process.exit(1);
};

const download = async (url, file) => {
  const res = await fetch(url, { headers: { "user-agent": "skopli-recapture" } });
  if (!res.ok) fail(`download ${url} answered ${res.status}`);
  mkdirSync(dirname(file), { recursive: true });
  const ws = createWriteStream(file);
  for await (const chunk of res.body) ws.write(chunk);
  await new Promise((r) => ws.end(r));
};
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
const waitFor = async (url, seconds) => {
  for (let i = 0; i < seconds; i++) {
    try {
      if ((await fetch(url)).ok) return true;
    } catch {}
    await sleep(1000);
  }
  return false;
};

async function env() {
  // home.mjs and reconcile.mjs read through the built facade and native addon.
  for (const a of [
    ["install", "--frozen-lockfile"],
    ["run", "build"],
  ])
    if (sh("pnpm", a, { cwd: root, stdio: "inherit" }).status !== 0)
      fail(`pnpm ${a.join(" ")} failed`);
  const bin = join(cache, `llama-${LLAMA.build}-${OS}`);
  let server =
    existsSync(bin) && findFile(bin, OS === "windows" ? "llama-server.exe" : "llama-server");
  if (!server) {
    const rel = await (
      await fetch(`https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/${LLAMA.build}`)
    ).json();
    const asset = rel.assets?.find((a) => LLAMA.asset[OS].test(a.name));
    if (!asset) fail(`no ${OS} asset in llama.cpp release ${LLAMA.build}`);
    const archive = join(cache, asset.name);
    if (!existsSync(archive)) {
      say(`download ${asset.name}`);
      await download(asset.browser_download_url, archive);
    }
    mkdirSync(bin, { recursive: true });
    const x =
      archive.endsWith(".zip") && OS === "linux"
        ? sh("unzip", ["-oq", archive, "-d", bin])
        : sh("tar", ["xf", archive, "-C", bin]);
    if (x.status !== 0) fail(`extract ${asset.name}: ${x.stderr}`);
    server = findFile(bin, OS === "windows" ? "llama-server.exe" : "llama-server");
    if (!server) fail(`llama-server not found in ${asset.name}`);
  }
  const model = join(cache, LLAMA.model);
  if (!existsSync(model) || statSync(model).size !== LLAMA.bytes) {
    say(`download ${LLAMA.model}`);
    await download(LLAMA.modelUrl, model);
  }
  if (statSync(model).size !== LLAMA.bytes)
    fail(`${LLAMA.model} is ${statSync(model).size} B, expected ${LLAMA.bytes} B`);
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
  ];
  const slog = openSync(join(out, "llama-server.txt"), "w");
  const srv = spawn(server, flags, { detached: OS !== "windows", stdio: ["ignore", slog, slog] });
  srv.unref();
  saveState({
    serverPid: srv.pid,
    os: OS,
    target,
    llama: { ...LLAMA, asset: undefined, flags: flags.slice(2) },
  });
  if (!(await waitFor(`http://127.0.0.1:${LLAMA.port}/health`, 300)))
    fail("llama-server not healthy after 300 s");
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
      detached: OS !== "windows",
      stdio: ["ignore", "ignore", openSync(join(out, "gateway-stderr.txt"), "w")],
    },
  );
  gw.unref();
  saveState({ gatewayPid: gw.pid });
  if (!(await waitFor(`${origin}/v1/models`, 60))) fail("gateway not answering after 60 s");
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

function stop() {
  const s = readJson(state, {});
  killTree(s.gatewayPid);
  killTree(s.serverPid);
  if (existsSync(state)) saveState({ serverPid: null, gatewayPid: null });
  say(`stop: ${s.serverPid ?? "-"} ${s.gatewayPid ?? "-"}`);
}

const which = (bin) =>
  sh(OS === "windows" ? "where" : "which", [bin], { noShell: true })
    .stdout?.split(/\r?\n/)[0]
    ?.trim() || null;
const binOf = (c) => (Array.isArray(c.first) ? c.first[0] : null);
function install() {
  mkdirSync(out, { recursive: true });
  const harnesses = [
    ...new Set(cells.filter((x) => x.plan !== "todo" || x.h.install).map((x) => x.harness)),
  ];
  const lines = [];
  for (const id of harnesses) {
    const { install: how, shapes } = manifest.harnesses[id];
    const bin = Object.values(shapes).map(binOf).find(Boolean);
    let r = null;
    if (how?.pnpm) r = sh("pnpm", ["add", "-g", how.pnpm]);
    else if (how?.uv) r = sh("uv", ["tool", "install", "--python", how.python, how.uv]);
    else if (how?.script && OS !== "windows")
      r = sh("bash", ["-c", `curl -fsSL ${how.script} | bash`]);
    if (r && r.status !== 0) say(`install ${id}: exit ${r.status}`);
    const path = bin && which(bin);
    const v = path ? sh(bin, ["--version"], { timeout: 25000 }) : null;
    lines.push(
      [
        id,
        path ? path.replace(userInfo().homedir, "~") : "MISSING",
        v ? (v.stdout || v.stderr).trim().split(/\r?\n/)[0] : "-",
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
const homeEnv = (home) =>
  Object.fromEntries(
    sh("bash", [join(here, "os-home.sh"), "--plain", home], { noShell: true })
      .stdout.trim()
      .split("\n")
      .map((l) => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1)]),
  );

const runTurn = (argv, envv, cwd, log, limit = timeout) =>
  new Promise((done) => {
    const t0 = Date.now();
    const child = spawn(argv[0], argv.slice(1), {
      cwd,
      env: envv,
      detached: OS !== "windows",
      stdio: ["ignore", "pipe", "pipe"],
      shell: OS === "windows",
    });
    let text = "";
    child.stdout.on("data", (d) => (text += d));
    child.stderr.on("data", (d) => (text += d));
    const timer = setTimeout(() => killTree(child.pid), limit * 1000);
    const finish = (code) => {
      clearTimeout(timer);
      const timedOut = Date.now() - t0 >= limit * 1000;
      writeFileSync(
        log,
        `cmd: ${argv.join(" ")}\n${text}\nexit: ${code}${timedOut ? " (timeout)" : ""}\n`,
      );
      done({ code, timedOut, text, seconds: Math.round((Date.now() - t0) / 1000) });
    };
    child.on("error", () => finish(127));
    child.on("close", (code) => finish(code ?? 137));
  });

const scan = (home, stores) => {
  const needles = [
    userInfo().username,
    hostname(),
    userInfo().homedir,
    "git-manager.devin.ai/proxy",
  ].filter((n) => n && n.length > 2);
  const hits = new Set();
  for (const s of stores)
    for (const f of files(join(home, s))) {
      const text = readFileSync(join(home, s, f)).toString("latin1");
      for (const n of needles)
        if (text.includes(n) || text.includes(n.replaceAll("\\", "\\\\")))
          hits.add(
            n === userInfo().username
              ? "username"
              : n === hostname()
                ? "hostname"
                : n === userInfo().homedir
                  ? "home-path"
                  : n,
          );
      if (/\b[A-Z0-9]{1,6}~\d\b/.test(text)) hits.add("8.3-alias");
    }
  return [...hits];
};

async function run() {
  const s = readJson(state, {});
  if (!s.gatewayPid) fail("no gateway recorded; run the env step first");
  for (const cell of cells) {
    const dir = join(out, "cells", cell.harness, cell.shape);
    const resultFile = join(dir, "result.json");
    if (args.flag("--resume") && existsSync(resultFile)) continue;
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
        status: cell.todo ? "todo" : "skip",
        reason: cell.skip ?? cell.todo,
        note: cell.c.note,
      });
      continue;
    }
    if (!which(binOf(cell.c))) {
      write({ status: "not-installed", reason: binOf(cell.c) });
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
      ...process.env,
      ...homeEnv(home),
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
    const gwlog = join(out, "gateway-usage.jsonl");
    const from = existsSync(gwlog) ? statSync(gwlog).size : 0;
    if (cell.c.setup)
      await runTurn(
        cell.c.setup.map((a) => fill(a, vars)),
        envv,
        cwd,
        join(dir, "setup.txt"),
      );
    const turns = [];
    const deadline = Date.now() + timeout * 1000;
    for (const [i, prompt] of manifest.prompts.entries()) {
      const left = Math.floor((deadline - Date.now()) / 1000);
      if (left < 20) {
        turns.push({ skipped: "cell budget spent" });
        continue;
      }
      const argv = (i ? cell.c.next : cell.c.first).map((a) => fill(a, { ...vars, prompt }));
      const t = await runTurn(argv, envv, cwd, join(dir, `run-${i + 1}.txt`), left);
      turns.push({ exit: t.code, timedOut: t.timedOut, seconds: t.seconds });
      if (!i && cell.h.session) vars.session = sessionId(cell.h.session, t.text, home) ?? "";
      if (t.timedOut) break;
    }
    const gw = existsSync(gwlog) ? readFileSync(gwlog).subarray(from).toString() : "";
    writeFileSync(join(dir, "gateway-usage.jsonl"), gw);
    const lines = gw
      .split("\n")
      .filter(Boolean)
      .map((l) => JSON.parse(l))
      .filter((l) => l.route);
    const node = (script, a) => sh(process.execPath, [join(here, script), ...a], { noShell: true });
    const hr = node("home.mjs", [home, "--out", dir]);
    const summary = readJson(join(dir, "home-result.json"), { events: 0, detected: [] });
    const rc = node("reconcile.mjs", [
      home,
      cell.harness,
      join(dir, "gateway-usage.jsonl"),
      "--quiet",
    ]);
    writeFileSync(join(dir, "reconcile.json"), rc.stdout ?? "");
    const events = summary.byHarness?.[cell.harness]?.events ?? 0;
    const stored = (cell.h.store ?? []).some((p) => existsSync(join(home, p)));
    const status =
      turns.some((t) => t.timedOut) && !events
        ? "hang"
        : !lines.length
          ? "capture-gap"
          : !events
            ? stored
              ? "reader-gap"
              : "capture-gap"
            : rc.status === 0
              ? "captured"
              : "captured-delta";
    write({
      status,
      reason:
        status === "capture-gap" && !lines.length ? "no request reached the gateway" : undefined,
      turns,
      gatewayLines: lines.length,
      events,
      sessions: summary.byHarness?.[cell.harness]?.sessions ?? 0,
      diagnostics: summary.diagnostics,
      detected: summary.detected,
      homeExit: hr.status,
      reconcileExit: rc.status,
      layout: layoutOf(
        files(home).filter((f) => (cell.h.store ?? []).some((p) => f.startsWith(p))),
      ),
      redaction: scan(home, cell.h.store ?? []),
    });
  }
}
// A store layout is its set of path shapes: any segment carrying an id, a date or a cwd slug becomes *.
const layoutOf = (paths) =>
  [
    ...new Set(
      paths.map((f) =>
        f
          .split("/")
          .map((seg) =>
            /\d/.test(seg) || /^-.*-$/.test(seg) ? `*${seg.match(/(\.[a-z]+)+$/)?.[0] ?? ""}` : seg,
          )
          .join("/"),
      ),
    ),
  ].sort();
const sessionId = (how, text, home) => {
  if (how.stdout) return text.match(new RegExp(how.stdout))?.[1] ?? null;
  const base = how.glob.split("/*")[0];
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
      status: "not-run",
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
    return existsSync(d) ? layoutOf(files(d)) : null;
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
    if (!ok && has && !["skip", "todo"].includes(r.status))
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
  }
  const base = args.one("--baseline");
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
  console.log(JSON.stringify({ out, os: OS, counts, drift: drift.length }, null, 2));
  return drift.length ? 3 : 0;
}

function pack() {
  const r = sh(
    "tar",
    [
      "czf",
      join(dirname(out), `${relative(dirname(out), out)}.tgz`),
      "--exclude=*/home/*/node_modules",
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
let code = 0;
try {
  for (const s of step === "all" ? ["env", "install", "run", "report", "package"] : [step])
    code = (await steps[s]()) || code;
} finally {
  if (step === "all") stop();
}
process.exit(code);

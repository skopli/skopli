---
name: verify-skopli
description: Drive Skopli the way a user does and capture proof. Covers the docs site (theme, search, nine-language code tabs, page actions, Markdown twins and llms feeds), the marketing site, and the library surface replayed against the golden corpus or a real home directory. Use before calling a web or facade change done, when a reviewer asks for runtime evidence, or when a claim about the sites needs a screenshot or response body.
---

# Verify Skopli

Skopli is one Rust core with facades in nine languages, plus two Astro sites: the marketing site (`web/site`, served at `/`) and the docs site (`web/docs`, served under `/skopli/`).
A user touches the two sites in a browser and the library through a facade.
This skill builds the real artifacts, serves the built output, drives one surface at a time, and leaves evidence behind.
Read `features/README.md` before driving; each feature file there is the recipe.
A change that moves a route, renames a control or adds a page updates the matching feature file in the same change, so the map stays the source of truth for what a proof covers.

Helpers live in `scripts/` and every path below is relative to the repository root.

## Launch

```sh
bash .agents/skills/verify-skopli/scripts/launch.sh
```

The script runs `pnpm install --frozen-lockfile`, `pnpm run build:native`, `pnpm --filter @skopli/docs build` and `pnpm --filter @skopli/site build`, then serves each `dist` with that package's own `astro preview` bound to `127.0.0.1`.
Serving the built output matters: search only exists after `pagefind` indexes `dist`, and the `.md` twins and `llms*.txt` feeds are build outputs too.
It refuses to start when `.verify/run/state.sh` exists or when a target port is already bound, so it never drives an instance another run owns.
Ready means the marketing root, the docs root and `/skopli/pagefind/pagefind-ui.js` all answer `200`; the script polls for 60 seconds, and after that prints both server logs, stops the servers through `cleanup.sh` and exits `1`.
It prints both URLs and PIDs and writes them to `.verify/run/state.sh`.

Defaults are the repository ports, `4321` for the site and `4322` for the docs.
Set `SKOPLI_SITE_PORT` and `SKOPLI_DOCS_PORT` to run beside another instance.

The library surface needs no server.
Launch builds the native addon into `native/` and `pnpm install` builds the TypeScript facade into `dist/`; `scripts/golden.mjs` then replays a golden case through them.
A Rust toolchain at or above the `rust-toolchain.toml` pin is required for the addon.
The first build of the addon takes about a minute; later launches reuse the cargo cache.

A headless Chromium is required for the browser steps.
`playwright-core` is a dev dependency and finds its own browser cache; when `scripts/browser.mjs` fails with a missing executable, run `pnpm exec playwright-core install chromium` once.

## Doctor

```sh
bash .agents/skills/verify-skopli/scripts/doctor.sh
```

Read-only.
It checks that both PIDs from `state.sh` are alive and own their ports, that the docs root serves the `Skopli docs` title and the inline theme script, that the search bundle and `llms.txt` answer, that both served `dist` roots exist and are newer than every file under `web/*/src`, and that `native/` and `dist/` hold the built library.
It prints `doctor: ok` and exits `0` only when every line passes.
Run it before the first drive and whenever a drive behaves unexpectedly; a stale `dist` is the usual cause of a false failure.

## Drive

Browser steps run through `scripts/browser.mjs`: one headless Chromium session per invocation, a step list on the command line, every result appended to `<out>/transcript.txt`.
Page requests to hosts other than `127.0.0.1` and `localhost` are blocked and logged, and a `fetch` step to such a host fails, so a drive proves the sites need no external network.
Clipboard permissions are granted to the local origins so the copy action can be read back.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/<feature> \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  eval "document.documentElement.dataset.theme" \
  click 'role=button[name="Switch to light theme"]' \
  screenshot light \
  reload \
  assert "document.documentElement.dataset.theme === 'light'"
```

Steps: `goto <url>`, `reload`, `viewport <w> <h>`, `click <selector>`, `fill <selector> <text>`, `press <selector> <key>`, `wait <selector>`, `text <selector>`, `eval <js>`, `assert <js>`, `fetch <url>`, `screenshot <name>`.
Selectors are Playwright selectors; prefer `role=` with an accessible name over CSS.
`eval` records the JSON result of the expression and awaits promises.
`assert` stops the run and saves `failure.png` when the expression is falsy, so a failed journey leaves its last screen behind.
`--no-js` before the steps disables JavaScript in the page; `eval` and `assert` still run, so a drive can prove what the markup alone delivers.
Run `node .agents/skills/verify-skopli/scripts/browser.mjs` with no steps to print the full step list.

Stable handles on the docs site:

| Handle                                                                        | Element                                                                                                       |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `role=button[name="Switch to light theme"]` / `[name="Switch to dark theme"]` | theme toggle in the header, label follows the current theme                                                   |
| `html[data-theme]`                                                            | current theme, set by an inline script before first paint, persisted in `localStorage["skopli-theme"]`        |
| `role=textbox[name="Search docs"]`                                            | docs search input once `pagefind-ui.js` has loaded; the name is the placeholder, there is no `searchbox` role |
| `role=button[name="Search"]`                                                  | search toggle, only visible at widths of 860px and below                                                      |
| `role=button[name="Menu"]`                                                    | sidebar toggle, only visible at widths of 860px and below                                                     |
| `role=tab[name="Python"]` and the other eight language names                  | code tabs, choice persisted in `localStorage["skopli-lang"]`                                                  |
| `role=button[name="Copy page"]`                                               | copies the page's Markdown twin, label flips to `Copied` for 1.5s                                             |
| `role=button[name="More actions"]`                                            | chevron joined to `Copy page`; `aria-expanded` is the menu state                                              |
| `role=menuitem[name="View as Markdown"]` and the other seven item names       | menu items, resolvable only while the menu is open                                                            |
| `nav[aria-label="Docs"]`                                                      | sidebar                                                                                                       |
| `nav[aria-label="Previous and next"]`                                         | pager                                                                                                         |

Routes: docs pages are `/skopli/<slug>/`, their twins `/skopli/<slug>.md`, the feeds `/skopli/llms.txt` and `/skopli/llms-full.txt`.
The marketing site is `/` with sections `#harnesses` and the same theme toggle and code tabs.

Library steps run through `scripts/golden.mjs`:

```sh
node .agents/skills/verify-skopli/scripts/golden.mjs claude/basic --out .verify/evidence/<run>/library
```

It reads `golden/claude/basic/input-manifest.json`, calls the facade's `readUsage` and `rollup` with the manifest's environment, writes `actual-events.json`, `actual-rollup.json` and `result.json` into the output directory, and exits `0` only when events and every rollup match the expected files.
Any `golden/<harness>/<case>/` with an `expected-events.json` is a valid case id.

To read a real or copied harness home instead of a golden case, use `scripts/home.mjs`:

```sh
node .agents/skills/verify-skopli/scripts/home.mjs <home-dir> --out .verify/evidence/<run>/library-home
```

It calls `detectHarnesses` and `readUsage` with that directory as `home` and `cwd` and an empty `env`, so nothing from the current machine's environment leaks in, and writes `home-result.json` (detected harnesses, event and session counts overall and per harness under `byHarness`, first diagnostic) next to `home-read.json`, the full `readUsage` result with its events, diagnostics and skipped files.
There is no expected output to compare against; the proof is the recorded counts and the absence of diagnostics.

## Usage gateway

Golden cases captured from a real harness need the harness to log the usage shape its cloud provider would send.
A local `llama-server` reports cache reuse in its own fields, so `scripts/gateway.ts` sits between the harness and the server and rewrites only the `usage` block on the way back.

```sh
node .agents/skills/verify-skopli/scripts/gateway.ts --port 8081 --upstream http://127.0.0.1:8080 --log .verify/evidence/<run>/gateway-usage.jsonl
```

It binds `127.0.0.1` on `--port` (default `8081`), forwards every request to `--upstream` (default `http://127.0.0.1:8080`) with its body and headers unchanged, streams responses through line by line, and prints one JSON line with `gateway` (its own origin), `upstream` and `pid` on stdout when it listens.
Stop it by that PID.
`--log` is optional and appends one JSON line per routed exchange (`at`, `path`, `status`, `stream`, `cacheControl`, `aborted`, `usage` as sent to the harness), which is the per-call evidence for a capture.
`aborted` is true when the harness closed the connection before the response finished, so a reconciliation can set those lines aside.
A harness whose base URL already ends in `/v1` posts bare `/messages` or `/chat/completions`; the gateway forwards routed paths to the upstream `/v1/...` endpoint, where `llama-server` serves them.
A refused or reset upstream connection is retried three times before the harness receives a `502`.

Command line: `--help` prints the options with examples, `--dry-run` prints the resolved configuration as JSON and exits, `--quiet` drops the per-request stderr lines, `--json` makes those lines JSON objects.
Progress and errors go to stderr; stdout carries only machine-readable JSON.
Exit codes: `0` ok, `1` could not listen, `2` usage error.

Two companion scripts turn a capture into evidence and a golden case.
`scripts/reconcile.mjs <home> <harness> <gateway-usage.jsonl>` reads the home through the facade, sums the gateway lines (aborted lines separately) and prints both totals with their delta; it exits `0` when input, output, cache read and cache write agree.
`scripts/promote.mjs <home> <harness>/<case> --store <rel>... --redact <from>=<to>... --cache <text>` copies the named stores into `golden/<harness>/<case>/input/`, rewrites text, JSON, zstd logs and SQLite text columns with the redaction pairs (longest source first, so a short rule cannot consume the path a longer rule names), writes `input-manifest.json` and a README naming the gateway, server, model and cache source, and regenerates the expected files through `scripts/replay.mjs`, the same replay `golden.mjs` uses.
Before writing the manifest it checks every file it copied and exits `1`, removing the case, when a redaction source is still present in the bytes (zstd logs are checked decompressed), when `.gitignore` would ignore a file, or when a non-text file has no `binary` attribute in `.gitattributes`; the message names the file and the fix.
`promote.mjs`, `reconcile.mjs` and `golden.mjs` share one command line shape through `scripts/cli.mjs`: `--help` prints the options with examples, `--dry-run` prints what the run would read and write as JSON and exits (for `promote.mjs` the resolved redaction rules and every file it would write, since it removes and rewrites the case directory), `--quiet` drops the progress and verdict lines on stderr, stdout carries only JSON, an unknown option exits `2`.
The test is `scripts/cli.test.ts`.
Name only the stores the reader consumes: object stores such as a harness's `snapshot/` directory carry machine identifiers in binary files that no redaction rule can rewrite.
A promoted set is proven from a clean checkout, not from the worktree that produced it: commit, `git worktree add --detach <dir> <sha>`, and run the golden sweep and the conformance tests there, because an ignored or eol-converted file is only missing or corrupt in the fresh tree.

Point a harness at it with the base URL variables it reads and a dummy key, for example `OPENAI_BASE_URL=http://127.0.0.1:8081/v1 OPENAI_API_KEY=sk-dummy` or `ANTHROPIC_BASE_URL=http://127.0.0.1:8081 ANTHROPIC_API_KEY=sk-ant-dummy`.
The key is never checked; confirm it does not appear in the promoted capture.

Cache mapping, with `llama-server` as the source of truth for what was reused from its prefix cache:

| Route                  | Server fields read                                               | Rewrite                                                                                                                                                                                                                                                                                                                                                                |
| ---------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/v1/chat/completions` | `timings.cache_n`                                                | `usage.prompt_tokens_details.cached_tokens = cache_n`; `prompt_tokens` stays the full prompt. In a stream the `timings` seen on any chunk apply to the chunk that carries `usage`.                                                                                                                                                                                     |
| `/v1/messages`         | `usage.cache_read_input_tokens`, `usage.input_tokens` (uncached) | `cache_read_input_tokens` kept; when a `system`, `tools` or message content block of the request carries `cache_control`, `cache_creation_input_tokens` = the uncached tokens and `input_tokens` = 0, otherwise `cache_creation_input_tokens` = 0 and `input_tokens` unchanged. In a stream `message_start` is rewritten and `message_delta.usage` is made cumulative. |
| `/v1/responses`        | none                                                             | forwarded unchanged; the server already fills `usage.input_tokens_details.cached_tokens`.                                                                                                                                                                                                                                                                              |

All three routes are server-reported: the chat and messages rewrites use the server's own counters, the Responses route passes the server's usage through, and nothing is derived from request text.
Run the server with prompt caching on (the default) and at least one slot per concurrent harness session, since a slot's prefix cache is what turn two reuses.
A capture's README names the gateway, the server build, the model and that the cache numbers are server-reported.
The unit test is `scripts/gateway.test.ts`, run by `pnpm run test`.

## Evidence

Evidence goes under `.verify/evidence/<run>/<feature>/`, where `<run>` is a UTC timestamp such as `20261018T180000Z`.
`.verify/` is ignored by git and survives cleanup.
A proof holds the action and the resulting state, never only the final screen:

- the `transcript.txt` from `browser.mjs`, which records each step, its result, console output and blocked requests;
- a screenshot before and after the action, taken by the `screenshot` step;
- the response body for anything fetched, saved by the `fetch` step;
- for the library, `result.json` with the match flags and the actual outputs next to it.

Proof standards: exercise the real user path (click the toggle, do not set `localStorage` by hand; click `Copy page`, do not fetch the `.md` yourself and call it copied).
Verify the side effect alongside the visible change: the storage key after a toggle, the clipboard text after a copy, the `hidden` attribute on the other panels after a tab click.
Nothing here is mocked; the sites make no external calls and the library reads only the golden input directory or the home directory named on the command line.
Record the feature id and the entry point used with every artifact, and report an unreachable path with the attempted command and the unmet precondition instead of substituting another path.

## Cleanup

```sh
bash .agents/skills/verify-skopli/scripts/cleanup.sh
```

It reads the PIDs from `.verify/run/state.sh`, confirms each command line is an `astro preview`, sends `SIGTERM`, waits up to ten seconds, sends `SIGKILL` to any server still alive, then removes `.verify/run/`.
It never kills by name and never touches `.verify/evidence/`.
Run it after every drive, including failed ones, so no preview server or port is left behind.
Build outputs (`web/*/dist`, `native/`, `dist/`) stay in place; they are ordinary gitignored build artifacts.

## Core arithmetic

This skill replays one golden case through the facade; it does not check that `rollup()` is right for inputs the corpus never saw.
For a change to `crates/skopli-core/src/rollup.rs`, `src/rollup.ts`, the shared timestamp parser or the ISO formatter, run `.agents/skills/verify-skopli-core/SKILL.md` as well.
It checks every input in a small finite domain against the documented promises and against the TypeScript twin in under a minute.

## Helpers

| Script                  | Invocation                                                                          | Purpose                                                               |
| ----------------------- | ----------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `scripts/launch.sh`     | `bash .agents/skills/verify-skopli/scripts/launch.sh`                               | build everything, serve both sites, write `state.sh`                  |
| `scripts/doctor.sh`     | `bash .agents/skills/verify-skopli/scripts/doctor.sh`                               | read-only health check, exit `0` when worth driving                   |
| `scripts/browser.mjs`   | `node .agents/skills/verify-skopli/scripts/browser.mjs --out <dir> <steps>`         | headless Chromium step runner with transcript, screenshots and bodies |
| `scripts/golden.mjs`    | `node .agents/skills/verify-skopli/scripts/golden.mjs <harness>/<case> --out <dir>` | replay a golden case through the facade and compare                   |
| `scripts/home.mjs`      | `node .agents/skills/verify-skopli/scripts/home.mjs <home-dir> --out <dir>`         | detect and read an arbitrary home directory through the facade        |
| `scripts/gateway.ts`    | `node .agents/skills/verify-skopli/scripts/gateway.ts --port <n> --upstream <url>`  | usage-rewriting proxy in front of a local `llama-server` for captures |
| `scripts/reconcile.mjs` | `node .agents/skills/verify-skopli/scripts/reconcile.mjs <home> <harness> <log>`    | compare a captured home's totals with the gateway log                 |
| `scripts/promote.mjs`   | `node .agents/skills/verify-skopli/scripts/promote.mjs <home> <harness>/<case> ...` | redact and copy a capture into a golden case and regenerate expected  |
| `scripts/cleanup.sh`    | `bash .agents/skills/verify-skopli/scripts/cleanup.sh`                              | stop the servers by PID, keep evidence                                |

The scripts are executable and run on Linux only: they depend on `bash`, `curl`, `ss`, `/proc`, `git`, Node 24 and the repository's dev dependencies.

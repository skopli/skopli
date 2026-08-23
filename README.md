<div align="center">

<!-- ASSET TODO: logo/banner -->

# Skopli

**Read AI coding-agent usage from 40 harnesses — offline — and price it against live market catalogs.**

[Website](https://skopli.com) · [Docs](https://docs.skopli.com/skopli/) · [Quickstart](#quickstart) · [Supported harnesses](#supported-harnesses) · [Why Skopli](#why-skopli)

<p>
  <img src="assets/logos/claude.svg" height="20" alt="Claude Code" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/opencode.svg" height="20" alt="OpenCode" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/codex.svg" height="20" alt="Codex CLI" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/gemini.svg" height="20" alt="Gemini CLI" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/copilot.svg" height="20" alt="Copilot CLI" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/amp.svg" height="20" alt="Amp" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/qwen.svg" height="20" alt="Qwen Code" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/goose.svg" height="20" alt="Goose" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/grok.svg" height="20" alt="Grok CLI" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/cline.svg" height="20" alt="Cline" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/roocode.svg" height="20" alt="Roo Code" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/kilocode.svg" height="20" alt="Kilo Code" align="absmiddle" />&nbsp;&nbsp;
  <img src="assets/logos/zed.svg" height="20" alt="Zed Agent" align="absmiddle" />
</p>

<!-- Badges render once the package is published / CI is public.
[![npm](https://img.shields.io/npm/v/skopli?labelColor=black)](https://www.npmjs.com/package/skopli)
[![downloads](https://img.shields.io/npm/dm/skopli?labelColor=black)](https://www.npmjs.com/package/skopli)
[![CI](https://img.shields.io/github/actions/workflow/status/skopli/skopli/ci.yml?labelColor=black)](https://github.com/skopli/skopli/actions)
[![license](https://img.shields.io/badge/license-MIT-blue?labelColor=black)](LICENSE)
-->

</div>

> No accounts, no telemetry. The readers never touch the network; only the pricing layer fetches (opt-in, cached, works offline from cache).

Skopli is an SDK, not a CLI. It reads the session state your agent harnesses already store on disk — tokens, sessions, turns, calls — normalizes it, aggregates it, and prices it. Build your own dashboards, reports, or billing checks on top.

## Why Skopli

Three independent layers; use any subset:

| Layer      | What it does                                                                                                                                                         | Network        |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| **Read**   | Parse local session state from 40 harnesses into normalized usage events, with structured diagnostics                                                                | Never          |
| **Rollup** | Group events by model, day (timezone-aware in every SDK), session, harness, workspace, or billing block                                                              | Never          |
| **Price**  | Match models against OpenRouter, LiteLLM, and models.dev catalogs (or your own) and compute USD costs — honest misses, tiered long-context rates, cache-write splits | Opt-in, cached |

Skopli is a library with an offline-first stance, per-event tiered pricing, and a stable API surface across TypeScript, Python, Rust, Ruby, Java, C#, Go, Swift, and C.

<!-- ASSET TODO: terminal recording or 3-layer architecture diagram -->

## Install

```
npm install skopli
```

`pnpm add skopli` and `bun add skopli` work too. Requires Node >= 24. ESM only.

<details>
<summary>Other languages (published with the first release)</summary>

| Language | Registry        | Install                                                          |
| -------- | --------------- | ---------------------------------------------------------------- |
| Python   | PyPI            | `pip install skopli`                                             |
| Rust     | crates.io       | `cargo add skopli-core`                                          |
| Ruby     | RubyGems        | `gem install skopli`                                             |
| Java     | Maven Central   | `com.skopli:skopli`                                              |
| C#       | NuGet           | `dotnet add package Skopli`                                      |
| Go       | Go modules      | `go get github.com/skopli/skopli/sdks/go`                        |
| Swift    | SPM             | `.package(url: "https://github.com/skopli/skopli", from: "...")` |
| C/C++    | GitHub Releases | prebuilt cdylib/staticlib + `skopli.h`                           |

</details>

### Building the C library

The C ABI (`skopli-capi`) builds to a cdylib (`skopli.dll` / `libskopli.so` /
`libskopli.dylib`) plus `include/skopli.h`. The default build is network-free: it
opens no socket and fetches no catalog. Callers pass pre-fetched catalogs. This
is the artifact the language facades embed.

```sh
# network-free default (the facades embed this)
cargo build -p skopli-capi --release
```

A bare-C consumer who wants zero-config built-in fetching (OpenRouter, LiteLLM,
models.dev) can opt in with the `net` cargo feature, which produces a
batteries-included `libskopli-full`:

```sh
# batteries-included, zero-config fetch for bare-C users
cargo build -p skopli-capi --release --features net
```

`-full` is a build-time choice, not a separate output path: both builds emit the
same file name per platform, so a self-hoster who wants the fetching build
renames or relocates the `--features net` output themselves. Without `net`, a
handle constructed with `builtinSources:true` returns the `NoBuiltinSources`
error (`AgStatus::Catalog`); with pre-fetched catalogs it prices offline.

## Quickstart

```ts
import { detectHarnesses, readUsage, rollup, createPricing } from "skopli";

// what harnesses have local data?
const { supported, unsupported } = detectHarnesses();

// read usage events (all detected harnesses, or pick explicitly)
const { events, diagnostics } = await readUsage({
  harnesses: ["claude", "opencode", "codex"],
  since: "2026-08-01",
});

// aggregate
const byModel = rollup(events, { by: "model" });
const byDay = rollup(events, { by: "day", tz: "Europe/London" });

// price against market catalogs
const pricing = createPricing();
const priced = await pricing.priceRollups(byModel);
for (const row of priced) {
  if (row.pricing.priced) {
    console.log(row.key, row.pricing.usd.toFixed(2), `(${row.pricing.source})`);
  } else {
    console.log(row.key, "unpriced; tried", row.pricing.attempted.join(", "));
  }
}
```

## Supported harnesses

40 supported. The id is what you pass in `readUsage({ harnesses: [...] })`.

| Harness                                                                                        | id               |     | Harness                                                                                          | id             |
| ---------------------------------------------------------------------------------------------- | ---------------- | --- | ------------------------------------------------------------------------------------------------ | -------------- |
| <img src="assets/logos/claude.svg" height="16" alt="" align="absmiddle" /> Claude Code         | `claude`         |     | <img src="assets/logos/grok.svg" height="16" alt="" align="absmiddle" /> Grok CLI                | `grok`         |
| <img src="assets/logos/opencode.svg" height="16" alt="" align="absmiddle" /> OpenCode          | `opencode`       |     | Augment                                                                                          | `augment`      |
| <img src="assets/logos/codex.svg" height="16" alt="" align="absmiddle" /> Codex CLI            | `codex`          |     | Codebuff                                                                                         | `codebuff`     |
| <img src="assets/logos/gemini.svg" height="16" alt="" align="absmiddle" /> Gemini CLI          | `gemini`         |     | Mux                                                                                              | `mux`          |
| <img src="assets/logos/copilot.svg" height="16" alt="" align="absmiddle" /> Copilot CLI (OTel) | `copilot`        |     | <img src="assets/logos/zed.svg" height="16" alt="" align="absmiddle" /> Zed Agent                | `zed`          |
| <img src="assets/logos/amp.svg" height="16" alt="" align="absmiddle" /> Amp                    | `amp`            |     | <img src="assets/logos/zcode.svg" height="16" alt="" align="absmiddle" /> ZCode                  | `zcode`        |
| Droid                                                                                          | `droid`          |     | <img src="assets/logos/devin.svg" height="16" alt="" align="absmiddle" /> Devin CLI              | `devin`        |
| <img src="assets/logos/qwen.svg" height="16" alt="" align="absmiddle" /> Qwen Code             | `qwen`           |     | <img src="assets/logos/roocode.svg" height="16" alt="" align="absmiddle" /> Roo Code             | `roo`          |
| Pi                                                                                             | `pi`             |     | <img src="assets/logos/cline.svg" height="16" alt="" align="absmiddle" /> Cline                  | `cline`        |
| OMP                                                                                            | `omp`            |     | <img src="assets/logos/kilocode.svg" height="16" alt="" align="absmiddle" /> Kilo Code (VS Code) | `kilocode`     |
| <img src="assets/logos/kilocode.svg" height="16" alt="" align="absmiddle" /> Kilo Code CLI     | `kilo`           |     | <img src="assets/logos/junie.svg" height="16" alt="" align="absmiddle" /> Junie                  | `junie`        |
| <img src="assets/logos/goose.svg" height="16" alt="" align="absmiddle" /> Goose                | `goose`          |     | <img src="assets/logos/openclaw.svg" height="16" alt="" align="absmiddle" /> OpenClaw            | `openclaw`     |
| Hermes                                                                                         | `hermes`         |     | <img src="assets/logos/kimi.svg" height="16" alt="" align="absmiddle" /> Kimi CLI                | `kimi`         |
| Prime Agent                                                                                    | `prime`          |     | gajae-code                                                                                       | `gajae`        |
| Kimchi Coding                                                                                  | `kimchi`         |     | MiMo Code                                                                                        | `mimocode`     |
| Command Code                                                                                   | `commandcode`    |     | CodeBuddy Code                                                                                   | `codebuddy`    |
| jcode                                                                                          | `jcode`          |     | Cherry Studio                                                                                    | `cherrystudio` |
| OpenCodeReview                                                                                 | `opencodereview` |     | Trae Agent                                                                                       | `trae`         |
| DeepSeek Harness                                                                               | `deepseek`       |     | Reasonix                                                                                         | `reasonix`     |
| Kiro CLI (estimated)                                                                           | `kiro`           |     | fx.sh                                                                                            | `fx`           |

Detected but unsupported (no reliable local token data): Cursor, aider, Crush, GitLab Duo CLI.
`detectHarnesses()` reports these separately so you can tell users why they are missing.

<sub>All product names, logos, and brands are property of their respective owners and are used for identification only. Their use is nominative and does not imply endorsement of or affiliation with Skopli. Icon attribution: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).</sub>

## Reading

```ts
const { events, diagnostics, skipped } = await readUsage({
  harnesses: ["claude"], // default: all detected
  since: "2026-08-01", // inclusive lower bound
  until: "2026-09-01", // exclusive upper bound
  tz: "America/Los_Angeles", // timezone for date-only bounds; default: system timezone
  subagents: "include", // "include" (default) | "exclude"
  home: "/custom/home", // optional PathResolver injection
  env: { CLAUDE_CONFIG_DIR: "/synced/claude" },
});
```

Date-only `since`/`until` strings (`YYYY-MM-DD`) are interpreted in `tz` (default: the system timezone): `since` means midnight at the start of that day, `until` means the exclusive end of that day.
Full ISO timestamps are used exactly as given.
This keeps filters and day rollups consistent west of UTC: with `tz: "America/Los_Angeles"`, `since: "2026-08-01"` excludes an event at `2026-08-01T00:30Z` (17:30 on Jul 31 in Los Angeles), so a day rollup in that timezone never shows a `"2026-07-31"` row. The native core and every facade bucket day rollups in `tz` natively (real IANA zones, including DST transitions and half-hour offsets), so this guarantee holds identically across all SDKs. An unknown zone name falls back to UTC.

Every event carries harness, timestamp (ISO), sessionId, messageId, turn flag, `subagent` flag, model name, token counts (input, output, cacheRead, cacheWrite, optional cacheWrite1h - the 1h-TTL portion of the cacheWrite total when the source reports the split - and reasoning), and an optional `calls` count for sources that aggregate multiple model calls per record.
Events also carry an optional `costUsd`: the harness-recorded cost when the source reports one. The Claude reader populates it from the jsonl `costUSD` field; the OpenCode reader from a nonzero `cost`. Other readers leave it absent.
Events also carry an optional `workspace` (normalized project directory path) and `title` (session title) when the source records them - OpenCode from its session directory/title, Claude Code from each record's `cwd`. Readers without the concept leave them undefined. Workspace paths are normalized cross-platform (forward slashes, no trailing separator, case-folded on Windows) so one repository is one key.

### Subagents

Subagent (child) sessions are real spend, so they are **included by default** and tagged rather than dropped. Pass `subagents: "exclude"` to drop them uniformly across every harness.

`event.subagent` is `true` when the harness's native marker says the event came from a subagent:

- **OpenCode**: the session has a non-null `parent_id` / `parentID` (SQLite and legacy JSON stores).
- **claude**: the JSONL entry has `isSidechain === true`.
- **pi** / **omp**: the session's `rlmDepth` is greater than 0.
- **codex**: subagent-attributable events where identifiable; codex exposes no reliable per-event marker today, so this is currently always `false`.
- every other harness: always `false` (no subagent concept).

The SDK never writes to stdio.
Problems (malformed lines, unreadable files, unusable timestamps) come back as structured `diagnostics`; skipped source files per harness come back in `skipped`.

## Rollups

```ts
rollup(events, { by: "model" });
rollup(events, { by: "day", tz: "Asia/Tokyo" }); // timezone-aware day bucketing
rollup(events, { by: "session" });
rollup(events, { by: "harness" });
rollup(events, { by: "workspace" }); // events without a workspace group under "(unknown)"
rollup(events, { by: "block", blockMs: 18_000_000 }); // rolling session windows (approximates a provider quota window)
```

## Pricing

```ts
const pricing = createPricing({
  // priority order; defaults shown
  sources: [openRouterSource(), liteLlmSource(), modelsDevSource()],
  // highest priority, beats every market source
  overrides: [{ model: "my-fine-tune", input: 5, output: 15 }],
  mode: "calculate", // "calculate" (default) | "display" | "auto"
  cacheDir: "/custom/cache", // default: platform cache dir
  offline: true, // cache only, never fetch
  ttlMs: 3_600_000, // cache freshness window
  refresh: true, // attempt one fresh fetch per source, bypassing fresh caches (stale cache still used if the fetch fails)
  fetch: myFetch, // injectable for tests/proxies
});

const hit = await pricing.lookupModel("openai/gpt-5");
// { priced: true, key: "gpt-5", price: {...}, source: "litellm", fetchedAt: "..." }
```

Prices are USD per million tokens. The highlights:

- **Offline-capable**: each catalog is cached on disk (default ~1 hour TTL, atomic writes); when a fetch fails, any-age stale cache is used. Default fetches time out after 10 seconds. `pricing.catalogs()` reports what loaded.
- **Honest misses**: model names match in tiers (exact key, provider-prefix strip, family aliases, separator/date normalization, provider-prefixed forms, alias table). Unmatched models return `{ priced: false, attempted: [...] }` - never guesses. Zero-cost catalog entries are treated as placeholders and skipped (`{ priced: false, reason: "zeroCost" }` when only they match); an explicit `$0` override is intentional and stays priced.
- **Real cost math**: reasoning tokens bill at the output rate; token buckets are disjoint (readers subtract reasoning-inclusive output at the source, so nothing bills twice); Claude's 5m/1h cache-write split bills at the correct per-TTL rates; long-context tier rates (128k-512k boundaries, LiteLLM catalog) are selected per request with marginal billing by default and whole-request repricing for Anthropic models.
- **Three cost modes**: `calculate` recomputes from tokens × rates, `display` reports the harness-recorded cost, `auto` prefers recorded and falls back to calculated.
- **Per-event tier correctness**: `priceRollups` prices summed rollups (exact for flat-rate models, flags `tieredAggregate: true` where summing could cross a tier boundary); `priceEvents` prices each event at its own context and then aggregates, which is the correct path for tiered models:

```ts
const priced = await pricing.priceEvents(events, { by: "model" });
```

The full pricing semantics — tier inheritance and fallbacks, the whole-request heuristic and its error bounds, mixed-provenance aggregation, cache-write composition per tier — are documented at [docs.skopli.com](https://docs.skopli.com/skopli/).

## Contributing

See the [development docs](https://docs.skopli.com/skopli/contributing) for the workspace layout (Rust core + 8 SDK facades), the golden-file test suite, and the local gate.

## License

MIT

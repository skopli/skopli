# Library golden replay

A developer calls `readUsage` and `rollup` from the TypeScript facade against a harness's local state and gets the events and rollups the golden corpus documents, with no network access.

## Sub-features

- `golden-read` returns the exact events in `golden/<harness>/<case>/expected-events.json` for the case's input directory.
- `golden-rollup` groups those events into the exact `expected-rollup.json` for every rollup dimension the case names.
- `golden-diagnostics` reports malformed rows as diagnostics and skipped files, not as thrown errors.
- `home-read` detects and reads whatever supported harness state sits under an arbitrary home directory, such as a copied real session store.

## How to get to it (user POV)

- `import { readUsage, rollup } from "skopli"` and call `readUsage({ harnesses: [...], env, home, tz })` then `rollup(events, { by })`.
- `import { detectHarnesses } from "skopli"` and call `detectHarnesses({ home })` to learn which harnesses have state under a home directory.
- `cargo test -p skopli-core --test conformance` replays the same corpus through the Rust core directly.

## Driving it with golden.mjs and home.mjs

Preconditions:

- Launch has run, so `native/` holds the addon and `dist/` the facade.
- A case directory such as `golden/claude/basic/` with an `expected-events.json`.
- For the home replay, a directory laid out like a user's home (for example `<dir>/.claude/projects/...`) that holds at least one native session file.

- **Replay a case.** Run `node .agents/skills/verify-skopli/scripts/golden.mjs claude/basic --out .verify/evidence/<run>/library`. Stderr ends with `golden: pass claude/basic`, exit code `0`, and `result.json` shows `"eventsMatch": true` with `model`, `day` and `harness` all `true` under `rollupsMatch`.
- **Second harness.** Run `node .agents/skills/verify-skopli/scripts/golden.mjs opencode/db-basic --out .verify/evidence/<run>/library-opencode`. Same shape of result; this case reads a SQLite store.
- **Diagnostics.** Read `result.json` from the first run. `diagnostics` is `2` and `skipped` is `2` for `claude/basic`, matching the malformed rows that case carries.
- **Rust core.** Run `cargo test -p skopli-core --test conformance` from the repository root and keep the output. It ends with `test result: ok`.
- **Real home.** Run `node .agents/skills/verify-skopli/scripts/home.mjs <home-dir> --out .verify/evidence/<run>/library-home` once per home directory. The helper exits `2` before reading anything when `<home-dir>` is not a directory. `home-result.json` lists the harness under `detected`, `sessions` and `events` are at least `1`, and `firstDiagnostic` is `null`. A harness whose state is present but not listed under `detected`, or listed with `events` at `0` while its session file carries usage, is a reader gap to report against the product, not a verification failure to paper over.
- **Proof.** `result.json` with every flag `true`, `actual-events.json` and `actual-rollup.json` next to it, the cargo test output, and one `home-result.json` per home directory replayed.

## Gotchas

- `golden.mjs` compares events and rollups only. Diagnostic messages carry absolute paths and are reported as counts, not compared.
- The facade needs `dist/index.js`; `pnpm install` builds it through the `prepare` script and `pnpm run build:native` builds the addon. A missing addon fails with `skopli native addon not found`.
- `golden/pricing/` holds pricing fixtures without `expected-events.json` and is not a valid case for this script.
- The manifest's `home` is set to a nonexistent path when the case does not name one, so the run never reads the real home directory.
- `home.mjs` passes an empty `env`, so environment overrides such as `CLAUDE_CONFIG_DIR` from the current shell never redirect a read; state that only an override would reach counts as not present.
- Two harnesses can share a sessions root (one is a fork of the other), in which case `detected` lists both and every event appears twice, once per harness name. Read the per-harness `events` and `sessions` under `byHarness` when that happens.

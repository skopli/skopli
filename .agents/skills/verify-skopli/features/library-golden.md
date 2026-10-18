# Library golden replay

A developer calls `readUsage` and `rollup` from the TypeScript facade against a harness's local state and gets the events and rollups the golden corpus documents, with no network access.

## Sub-features

- `golden-read` returns the exact events in `golden/<harness>/<case>/expected-events.json` for the case's input directory.
- `golden-rollup` groups those events into the exact `expected-rollup.json` for every rollup dimension the case names.
- `golden-diagnostics` reports malformed rows as diagnostics and skipped files, not as thrown errors.

## How to get to it (user POV)

- `import { readUsage, rollup } from "skopli"` and call `readUsage({ harnesses: [...], env, home, tz })` then `rollup(events, { by })`.
- `cargo test -p skopli-core --test conformance` replays the same corpus through the Rust core directly.

## Driving it with golden.mjs

Preconditions:

- Launch has run, so `native/` holds the addon and `dist/` the facade.
- A case directory such as `golden/claude/basic/` with an `expected-events.json`.

- **Replay a case.** Run `node .agents/skills/verify-skopli/scripts/golden.mjs claude/basic --out .verify/evidence/<run>/library`. The output ends with `golden: pass`, exit code `0`, and `result.json` shows `"eventsMatch": true` with `model`, `day` and `harness` all `true` under `rollupsMatch`.
- **Second harness.** Run `node .agents/skills/verify-skopli/scripts/golden.mjs opencode/db-basic --out .verify/evidence/<run>/library-opencode`. Same shape of result; this case reads a SQLite store.
- **Diagnostics.** Read `result.json` from the first run. `diagnostics` is `2` and `skipped` is `2` for `claude/basic`, matching the malformed rows that case carries.
- **Rust core.** Run `cargo test -p skopli-core --test conformance` from the repository root and keep the output. It ends with `test result: ok`.
- **Proof.** `result.json` with every flag `true`, `actual-events.json` and `actual-rollup.json` next to it, and the cargo test output.

## Gotchas

- `golden.mjs` compares events and rollups only. Diagnostic messages carry absolute paths and are reported as counts, not compared.
- The facade needs `dist/index.js`; `pnpm install` builds it through the `prepare` script and `pnpm run build:native` builds the addon. A missing addon fails with `skopli native addon not found`.
- `golden/pricing/` holds pricing fixtures without `expected-events.json` and is not a valid case for this script.
- The manifest's `home` is set to a nonexistent path when the case does not name one, so the run never reads the real home directory.

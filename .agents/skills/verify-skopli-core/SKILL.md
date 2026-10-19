---
name: verify-skopli-core
description: Bounded exhaustive check of the Rust rollup core in crates/skopli-core/src/rollup.rs against its documented promises and the TypeScript facade. Use before calling a change to rollup.rs, src/rollup.ts, the shared timestamp parser or the ISO formatter done, and whenever a reviewer asks whether a rollup can lose, invent or misfile a token.
---

# Verify Skopli core

`rollup()` is the one function every Skopli number passes through: the CLI, the wire format and all nine facades hand it a list of usage events and print what comes back.
If it drops an event, counts a token twice, lets a cache split exceed its parent counter, or files an event in the wrong row, every report built on Skopli is wrong and nothing downstream can tell.
This skill checks that it does none of those things, for every input in a small finite domain, and that the TypeScript twin in `src/rollup.ts` returns the same rows.

## Run

From the repository root, with the pinned Rust toolchain and Node 24 on `PATH`:

```sh
cargo test --release --manifest-path .agents/skills/verify-skopli-core/Cargo.toml
```

The tool is `cargo test` on the toolchain pinned by `rust-toolchain.toml` (Rust 1.89.0).
Node 24 is required because one test runs `src/rollup.ts` directly through Node's TypeScript support.
The run takes under a minute after the first build and about ten seconds of that is the check itself.
The Rust core is a path dependency, so the check always runs against the working tree.
The crate has its own `Cargo.lock`, seeded from the root `Cargo.lock` so the harness builds the core against the same `jiff` and bundled tzdb that ship; when the root lock changes, copy it over this one and let `cargo` prune it on the next run.

## What is checked

Three tests live in `tests/rollup.rs`; the domain and the properties live in `src/lib.rs`.

`rollup_keeps_its_promises_on_every_case` enumerates every sequence of zero to three events over the domain below and every rollup dimension, calls the real `rollup()`, and checks three promises:

1. Conservation.
   Every token counter, the clamped `cacheWrite1h`, `events`, `turns`, `calls` (absent means one) and finite `costUsd` sum to exactly the same value over the rows as over the input below `u64::MAX` (past it the rows sit at the ceiling, see the saturating rule below), and no row has `cacheWrite1h` above its `cacheWrite`.
2. Keyed rows.
   For `model`, `session`, `harness`, `workspace` and `day`, the row keys are the distinct expected keys of the input in ascending byte order, and each row holds exactly the events with its key: conservation from item 1 is re-run per row over those events alone, `cacheWrite1h` is present on a row exactly when one of its events carried it, and `costUsd` exactly when one of its events carried a finite cost.
   The expected key is the event's own field for the four named dimensions and a literal calendar table in `INSTANTS` for `day`, so nothing in the expectation comes from the crate under test.
   Every event carries a different value in each of `harness`, `session_id`, `model` and `workspace` (`hb`, `sb`, `mb`, `wb` for key `b`), so a core that keys one dimension on another field fails rather than passing by coincidence.
   For `day`, an unknown zone name is expected to give the UTC calendar date, the fallback `rollup.rs` documents.
3. Blocks.
   For `block`, the row keys equal an executable spec written from `golden/rollup-block/README.md`: sort by instant, open a block at the first event floored to the block grain (the width below one hour, the hour otherwise), keep events while `ms < start + width`, send unparseable instants to one trailing `invalid-date` row, fall back to `18000000` ms for a missing or non-positive width.
   The spec also names each block's member events, and every row gets the same per-row conservation and presence checks as a keyed row, so an event filed in the wrong block fails even when the counts per block come out right.
   The spec takes each instant's epoch milliseconds and hour-floor key from the same literal table, so a parser or formatter regression in the core shows up as a spec mismatch.
   A sub-hour start is the literal hour with the whole grains past it written into the minute, second and millisecond fields by the spec itself, so no key text comes from the crate under test.
   Block keys must also be strictly ascending.
   The README promises only ascending order; the distinct-key promise comes from the `Rollup` type, which has no identity other than `key`, so a consumer that keys rows by it merges or overwrites two rows with the same key.

`rollup_agrees_with_the_typescript_facade` pipes the same cases over stdin to `src/rollup.ts` and requires identical JSON rows, numbers compared as floats.
Non-finite costs travel as the strings `"NaN"` and `"Infinity"` and are revived to numbers before the TypeScript rollup runs, so its finite filter sees real non-finite values.

`counters_near_u64_max_do_not_wrap` is one directed case: two events whose `input` counters together exceed `u64::MAX`.

## Bounds

- At most three events per case, with repetition, so 107,100 cases in total.
- Keyed dimensions vary over three keys and eight token, calls, cost and turn shapes at one instant.
- `day` and `block` vary over eight instants around the UTC hour, day and five-hour edges and the New York midnight (`04:59:59.999Z` and `05:00:00.000Z` on a date where New York is on EST), one unparseable instant, and two shapes, in `UTC`, `America/New_York` and the unknown zone `Not/A_Zone` for `day` and `UTC` for `block`.
- Block widths: absent, zero, one hour, ninety minutes and thirty minutes.
- Counters go up to `2^40` so every sum is exact in a 64-bit float on the TypeScript side.
- Costs are dyadic fractions plus `Infinity` and `NaN`, so sums are exact and the finite filter is exercised.

Not covered: workspace fallback to a non-workspace key, an omitted `tz` (the system zone, which depends on the machine), negative epochs, offsets other than `Z`, more than three events, non-UTC block zones, daylight-saving transitions, and the readers that produce events.

## Assumptions

- The repository's `rust-toolchain.toml` is honoured and Node is version 24 or newer.
- `src/rollup.ts` is the behavioural twin of `rollup.rs`; a disagreement is a finding in one of them, and the skill does not say which.
- The block spec in `src/lib.rs` is a faithful reading of `golden/rollup-block/README.md`; when the two disagree, read the README first.

## Real bug or modeling error

A failing case prints the options, the input events, the rows the core produced and, for parity, the TypeScript rows.
Rerun the printed events by hand through `rollup()` before deciding.

It is a real bug when the failure is in `rollup.rs` or `rollup.ts` and the documented behaviour is clear: a counter that does not add up, a row with the wrong key, a block boundary off by one, or Rust and TypeScript disagreeing on the same input.

It is a modeling error when `src/lib.rs` disagrees with the documentation or the golden corpus: the block spec was read wrong, a domain value is outside what the core promises to handle, or the expected presence rule for an optional field was stated too strictly.
Fix the model, state why in the commit, and rerun.

The check is green: 107,100 cases, zero findings, parity with the TypeScript facade.
Two earlier findings are closed in the core and pinned here: block starts below one hour are floored to the width, so no two blocks share a key, and every counter saturates at `u64::MAX` instead of wrapping.
`check_conservation` states the saturating rule as `conserved`: rows total exactly the events below the ceiling; past it the rows total at least the ceiling and never more than the true sum.
`counters_near_u64_max_do_not_wrap` drives one row past the ceiling and expects it to sit there.

## Proofs

`src/kani_proofs.rs` proves the pure integer kernels of `rollup.rs` for every input, not only the enumerated domain, with Kani.
The kernels are `add_tokens`, `block_start` and `block_end`; each is plain integer arithmetic with no allocation, string or map.
Install with `cargo install --locked kani-verifier && cargo kani setup`, then run one harness at a time from the repository root:

```sh
timeout 600 cargo kani --manifest-path .agents/skills/verify-skopli-core/Cargo.toml --harness <name>
```

| Harness                                                     | Proves                                                                                                                                                                                      | Inputs                                                                                 |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `add_tokens_saturates_and_keeps_the_split_under_the_writes` | every counter is the saturating sum, the 1h split is clamped to its own event's writes first, and a row whose split was within its writes stays so                                          | any two `TokenCounts`                                                                  |
| `block_start_is_within_one_grain_before_the_event`          | the start is at most the event, less than one grain before it, inside the local hour, equal to the hour start at a width of an hour or more, and the event lies in `[start, end)`; no panic | any instant in V8's range, any positive width, any `[0, HOUR_MS)` offset into the hour |
| `block_start_is_aligned_to_the_grain_for_the_pinned_widths` | the start is a whole number of grains past the local hour start                                                                                                                             | any instant in V8's range, any offset into the hour, each width in `PINNED_WIDTHS`     |
| `later_block_starts_strictly_after_the_open_one`            | an event at or past the open block's end opens a block with a strictly greater start, so block keys are distinct and ascending                                                              | two such instants, one width, two offsets                                              |

The offset into the hour is left free in `[0, HOUR_MS)` rather than derived from UTC, so the block proofs hold for every zone's wall clock.
`PINNED_WIDTHS` is one millisecond, thirty minutes, forty minutes, one millisecond under the hour, the hour, ninety minutes and five hours: the widths the golden corpus and the exhaustive harness use plus the edges of the sub-hour range.

Per function:

- `add_tokens`: proved by Kani for any two inputs.
- `block_end`: proved by Kani (no panic, event inside `[start, end)`) for any in-range instant and positive width.
- `block_start`, containment and bound (`hour_start <= start <= ms`, `ms - start < grain`, hour start at a width of an hour or more): proved by Kani for any in-range instant, any positive width and any offset into the hour.
- `block_start`, distinct ascending keys (`next_start > start` when the next event is at or past the open block's end): proved by Kani for the same inputs.
- `block_start`, alignment of the start to a grain boundary (`(start - hour_start) % grain == 0`): proved by Kani only for the seven pinned widths; for a symbolic width it is enumerated, by the exhaustive harness (the thirty-minute width over the `:30` and `:59:59.999` instants, where an unfloored start has a different key) and by the golden cases `sub-hour-width-floors-to-the-width`, `forty-minute-width-floors-from-the-hour` and `forty-minute-width-key-inside-previous-window`.
  The containment and bound harness alone does not establish alignment: a mutant that returns the instant itself below one hour passes it and the ascending-keys harness, and fails the pinned-widths harness (`1 of 149 failed`, 0.6 s), the exhaustive harness (3,994 of 107,100 cases) and the golden conformance tests.
- `rollup`, `rollup_key`, `day_key`, `within_hour`, `to_iso_string`: enumerated by the exhaustive harness over its domain, not proved.

Symbolic-width alignment was attempted four ways, each with `timeout 600`; all four ended at the 600 s wall clock (`exit 124`) with CBMC still in `Running propositional reduction` after the SAT instance was built, so the limit is the solver, not an unsupported construct:

- `(start - hour_start) % grain == 0` as an `i64` remainder: 22,721 variables, 102,430 clauses.
- the same remainder narrowed to `u32` (sound, since `0 <= start - hour_start < HOUR_MS`): 16,666 variables, 67,910 clauses.
- a quotient witness `k` with `k * grain <= within < (k + 1) * grain`, asserting `start == hour_start + k * grain`: 66,007 variables, 341,451 clauses.
- same cell, same start: for any `ms2` in `[start, ms]`, `block_start(ms2, within - (ms - ms2), width) == start`: 24,484 variables, 106,241 clauses.

Settings: no stubs, default solver (CaDiCaL), no unwind bound except `#[kani::unwind(8)]` on the pinned-widths harness for its seven-iteration loop.
Last run: Kani 0.68.0 with CBMC 6.11.0 and CaDiCaL 3.0.0; the three single-shot harnesses verify in about one second of solver time each (231, 70 and 64 checks) and the pinned-widths harness in about twenty-two seconds (154 checks).
The ascending-keys harness fails against the former hour-floor rule, which was checked by running it once with that rule restored.

The formatting of a start into its key is `to_iso_string`, which allocates and is outside the kernels; it is pinned by the core's tests against literal V8 output and by the exhaustive check above.

## When the verified code changes on purpose

- A new rollup dimension: add it to the `by` list in `cases()` and a matching arm to `expected_key`.
- A new counter on `TokenCounts` or `Rollup`: add it to `Pattern`, `event()`, the `pairs` table in `check_conservation` and the parity JSON if it is optional.
- A change to the block rule or the default width: update `block_spec` and `golden/rollup-block/README.md` in the same change, in that order of authority.
- A change to the timestamp grammar: add the new accepted and rejected forms to `INSTANTS`, each with its epoch milliseconds, hour floor and day keys worked out by hand.
- A new day zone: add a column to `Instant` and an arm to `expected_key`; a change to the unknown-zone fallback changes the `Not/A_Zone` arm instead.
- A non-UTC block zone: `block_spec` floors in UTC and `Instant.hour` holds the UTC hour, so add a per-zone hour column and pass the zone into `block_spec` before adding the zone to `cases()`; until then such a case fails as a modeling error, not a finding.
- A change to the clamp, the finite-cost rule or the absent-calls default: update the matching line in `check_conservation` and say in the commit which promise changed.

Keep the domain small enough that a run stays under a minute; widen a single dimension rather than all of them.

## Limits

The exhaustive check covers every input in the stated domain and says nothing about inputs outside it; only the three Kani harnesses speak for all inputs, and only for the kernels they name.
It does not check the readers, pricing, the wire format or any facade other than TypeScript.

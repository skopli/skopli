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
The run takes under a minute after the first build and about five seconds of that is the check itself.
The crate has its own `Cargo.lock`; the Rust core is a path dependency, so the check always runs against the working tree.

## What is checked

Three tests live in `tests/rollup.rs`; the domain and the properties live in `src/lib.rs`.

`rollup_keeps_its_promises_on_every_case` enumerates every sequence of zero to three events over the domain below and every rollup dimension, calls the real `rollup()`, and checks three promises:

1. Conservation.
   Every token counter, the clamped `cacheWrite1h`, `events`, `turns`, `calls` (absent means one) and finite `costUsd` sum to exactly the same value over the rows as over the input, and no row has `cacheWrite1h` above its `cacheWrite`.
2. Keyed rows.
   For `model`, `session`, `harness`, `workspace` and `day`, the row keys are the distinct keys of the input in ascending byte order, each row holds exactly the events with its key, and `cacheWrite1h` and `costUsd` are present on a row exactly when one of its events carried them.
3. Blocks.
   For `block`, the rows equal an executable spec written from `golden/rollup-block/README.md`: sort by instant, open a block at the first event floored to the hour, keep events while `ms < start + width`, send unparseable instants to one trailing `invalid-date` row, fall back to five hours for a missing or non-positive width.
   Block keys must also be on the hour and strictly ascending.

`rollup_agrees_with_the_typescript_facade` sends the same cases through `src/rollup.ts` and requires identical JSON rows, numbers compared as floats.

`counters_near_u64_max_do_not_wrap` is one directed case: two events whose `input` counters together exceed `u64::MAX`.

## Bounds

- At most three events per case, with repetition, so 78,385 cases in total.
- Keyed dimensions vary over three keys and eight token, calls, cost and turn shapes at one instant.
- `day` and `block` vary over seven instants around the hour, day and five-hour edges, one unparseable instant, and two shapes, in `UTC` and `America/New_York` for `day` and `UTC` for `block`.
- Block widths: absent, zero, one hour, ninety minutes and thirty minutes.
- Counters go up to `2^40` so every sum is exact in a 64-bit float on the TypeScript side.
- Costs are dyadic fractions plus `Infinity` and `NaN`, so sums are exact and the finite filter is exercised.

Not covered: workspace fallback to a non-workspace key, negative epochs, offsets other than `Z`, more than three events, non-UTC block zones, and the readers that produce events.

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

The check currently fails on two real findings and on nothing else:

- `block keys repeat or go backwards`: a block width below one hour yields several rows with the same key, because every block start is floored to the hour while the window is shorter than an hour.
  Two events at `22:30` with a thirty-minute width produce two `22:00` rows.
  Rust and TypeScript agree, so the parity test stays green.
- `counters_near_u64_max_do_not_wrap`: the core adds `u64` counters unchecked, so a sum past `u64::MAX` panics in a debug build and wraps in a release build.

When these are fixed in the core, the run goes fully green and this section should shrink to say so.

## When the verified code changes on purpose

- A new rollup dimension: add it to the `by` list in `cases()` and extend `rollup_key` coverage in `check_keyed`.
- A new counter on `TokenCounts` or `Rollup`: add it to `Pattern`, `event()`, the `pairs` table in `check_conservation` and the parity JSON if it is optional.
- A change to the block rule: update `block_spec` and `golden/rollup-block/README.md` in the same change, in that order of authority.
- A change to the timestamp grammar: add the new accepted and rejected forms to `INSTANTS`.
- A change to the clamp, the finite-cost rule or the absent-calls default: update the matching line in `check_conservation` and say in the commit which promise changed.

Keep the domain small enough that a run stays under a minute; widen a single dimension rather than all of them.

## Limits

This is bounded exhaustive checking, not a proof over all inputs.
It covers every input in the stated domain and says nothing about inputs outside it.
It does not check the readers, pricing, the wire format or any facade other than TypeScript.

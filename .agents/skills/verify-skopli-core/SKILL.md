---
name: verify-skopli-core
description: Bounded exhaustive checks of the Rust core in crates/skopli-core - the rollup against its documented promises and the TypeScript facade, every reader against damaged and shifted input, and harness detection against an enumerated marker table in both Rust and TypeScript. Use before calling a change to rollup.rs, src/rollup.ts, a reader, the detector in src/detect.ts or skopli-wire detect done, and whenever a reviewer asks whether a rollup can misfile a token, a reader can crash or drop a record silently, or a harness can go undetected.
---

# Verify Skopli core

`rollup()` is the one function every Skopli number passes through: the CLI, the wire format and all nine facades hand it a list of usage events and print what comes back.
If it drops an event, counts a token twice, lets a cache split exceed its parent counter, or files an event in the wrong row, every report built on Skopli is wrong and nothing downstream can tell.
The events come from forty readers in `crates/skopli-core/src/readers/`, each parsing a harness's own session files, and the harnesses come from two detectors that must agree on which homes hold which harness.
This skill checks the rollup for every input in a small finite domain and parity with the TypeScript twin in `src/rollup.ts`, every reader against every single-cell and byte-level damage of its golden input, and both detectors against every layout of zero, one or two markers.

## Run

From the repository root, with the pinned Rust toolchain and Node 24 on `PATH`:

```sh
cargo test --release --manifest-path .agents/skills/verify-skopli-core/Cargo.toml
```

The tool is `cargo test` on the toolchain pinned by `rust-toolchain.toml` (Rust 1.89.0).
Node 24 is required because one test runs `src/rollup.ts` and one runs `src/detect.ts` directly through Node's TypeScript support.
The run takes under a minute after the first build: the rollup check about eight seconds, the reader checks about two, the detection checks about five.
The Rust core and the wire crate are path dependencies, so the check always runs against the working tree.
The crate has its own `Cargo.lock`, seeded from the root `Cargo.lock` so the harness builds the core against the same `jiff` and bundled tzdb that ship; when the root lock changes, copy it over this one and let `cargo` prune it on the next run.

To see the run counts each check prints, add `-- --nocapture`.
To run one target, add `--test rollup`, `--test readers` or `--test detect`.

## Rollup

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

## Readers

Three tests live in `tests/readers.rs`; the mutation and judgement helpers live in `src/readers.rs`.
The domain is the golden corpus: every `golden/<harness>/<case>/` with an `input-manifest.json`, forty-five cases over all forty registered readers, copied into a scratch home and read through the real `Reader::read` with the context the manifest describes, exactly as `crates/skopli-core/tests/conformance.rs` builds it.
Nothing in the expectations comes from the reader under test: the token classes come from the cell names in the harness's own files, the event counts from `expected-events.json`, the timestamp grammar from the shared parser.

`readers_never_panic_on_damaged_input` is the totality check.
For every file of every case it writes every variant below, runs the reader, and fails on a panic.
A reader may return an event whose timestamp no longer parses; when it does, the check runs the wire read the facades call (`skopli_wire::read::read_usage`) on the same home and requires that exactly those events are dropped, each with an `unusable timestamp` diagnostic, so the facades never see one.

- Every JSON leaf of every `.json`, `.jsonl` and `.ndjson` document, in turn, replaced by `null`, `""`, `"x"`, `-1`, `1e300`, `2^64`, `true`, `{}`, `[]`, and removed.
- Every file, JSON or not, replaced by the empty file, its first byte, its first half, all but its last byte, a lone `{`, and five bytes of binary.

`token_cells_reach_their_own_event_field` is the mapping check.
A token cell is a leaf whose name says which counter it is (`input`/`prompt`/`cachemiss`, `output`/`completion`/`candidates`, `cache` split into a write when the path says `write` or `creat` and into the 1h write when the leaf says `1h`, otherwise a read, `reason`/`thought`); a leaf named `tokens` takes its parent's name (`input.tokens`), and cells whose path says `total`, `cost` or `price` are derived and skipped.
A cell is live when removing its record (the deepest array element on its path, or the whole document) changes the events; inert cells are skipped and counted.
For each live cell the check bumps the cell by one, keeping any sibling `total` consistent so a reader that reads the total to decide whether the input includes cached tokens keeps its reading, and overwrites the cell with text.
The bump must move the class's own counter (`input`, `output`, `cacheRead`, `cacheWrite`, `cacheWrite1h` or `reasoning`, one class per counter) by exactly one, may also move the one counter readers derive by subtracting that class (`input` for a cache cell, since `prompt` includes cached tokens; `output` for a reasoning cell, since `completion` includes reasoning) by exactly minus one, and may move nothing else; or it must turn the record into a diagnostic.
A bump that moves any other counter, moves the derived counter the wrong way, or leaves the own counter unmoved is a dimension swap; one that moves the own counter by anything but one is a scaling; one that moves nothing and raises nothing is an ignored cell.
The text overwrite may keep or drop the record, but a drop must raise a diagnostic.
Three literal tables in the test keep the check honest:

- `IGNORED_BY_DESIGN` names the live cells a reader reads around on purpose, each with the reason from the reader's own documentation (amp per-message usage behind a ledger, the claude 5m split, the one negative claude 1h split that the reader drops as absent, the deepseek chunk sample, the fx snapshot totals, prime child usage, the reasonix prompt total).
  A row matches on `path=value`, so the negative-split row matches only the negative cell and the positive 1h cells are checked.
  An entry that matches no cell fails the check, so the list cannot outlive the corpus.
- `KNOWN_SILENT_DROPS` names the findings the check makes in the product today (below).
  An entry that stops reproducing fails the check, so the list cannot outlive its fix.
- `NO_MAPPING_COVERAGE` names the readers whose golden input holds no token cell the check can reach.
  The check fails when that list differs from the set of readers with no live mutation, in either direction, so a reader cannot lose or gain coverage unnoticed.

`readers_are_deterministic_and_timestamps_round_trip` reads each case twice and requires identical events, skipped and warnings; the event count must be the golden one; every timestamp must parse, format back through `to_iso_string` and parse to the same instant; every cost must be finite.

### Bounds

- One mutation per run: a single cell replaced or a single file damaged, never two at once.
- 15,905 damaged inputs across the forty-five cases, 92 of which leave the reader with an unusable timestamp that the wire read drops.
- 630 live token cell mutations over 31 readers, 91 inert cells skipped, 11 ignored-by-design entries exercised, 4 known silent drops reproduced.
- Files the readers open through other means (SQLite databases, zstd streams) get only the byte-level damage, since they hold no JSON leaves to shift.

Not covered: the token mapping of the nine readers in `NO_MAPPING_COVERAGE` (`cherrystudio`, `goose`, `hermes`, `kilo`, `kiro` and `zed` read SQLite only; `cline`, `roo` and `kilocode` keep their counters inside a JSON string in `ui_messages.json`, which the leaf walk does not enter), two faults in one record, faults in file names or directory structure, records the golden case does not contain, readers' handling of a cell type they never see in the corpus, and the exact diagnostic text.

### Assumptions

- The golden corpus is a faithful sample of each harness's record grammar; a cell shape absent from it is not exercised.
- A cell name tells its class, including which side of the cache it is on: a bare `cache`, `cached` or `cacheInputTokens` is a read in every reader of the corpus.
  A harness that names an input counter `output` would be flagged as a swap, and the fix would be an `IGNORED_BY_DESIGN` entry with the harness's documentation as the reason.
- The wire read is the only path the facades take to the readers, so a diagnostic there counts as reported.

### Findings

The check finds four silent drops in the product today, pinned in `KNOWN_SILENT_DROPS` and left for the reader owners to fix:

| Reader | Record                         | Mutation                                           | Behaviour                                |
| ------ | ------------------------------ | -------------------------------------------------- | ---------------------------------------- |
| amp    | a `usageLedger.events[]` entry | `tokens.input` or `tokens.output` set to text      | the entry is skipped with no diagnostic  |
| grok   | a unified log `ctx` record     | `prompt_tokens` or `completion_tokens` set to text | the record is skipped with no diagnostic |

Every other live cell in the corpus either keeps its record or loses it with a diagnostic under the same mutation.

### Broken variants

The check was shown to catch each of these deliberate breaks, applied one at a time to the working tree and reverted after the run:

| Break                                                                             | Where               | Caught by                                                           |
| --------------------------------------------------------------------------------- | ------------------- | ------------------------------------------------------------------- |
| swap `input` and `output` in the claude token struct                              | `readers/claude.rs` | mapping: `dimension swap in claude/basic (22 runs)`, 28 of 630 runs |
| swap `input` and `cache_read` in the claude token struct                          | `readers/claude.rs` | mapping: `dimension swap in claude/basic (21 runs)`, 26 of 630 runs |
| swap `cache_read` and `cache_write` in the claude token struct                    | `readers/claude.rs` | mapping: `dimension swap in claude/basic (20 runs)`, 24 of 630 runs |
| report a gemini record with unreadable tokens as well-formed (`malformed: false`) | `readers/gemini.rs` | mapping: `silent drop in gemini/basic (9 runs)`                     |
| `unwrap()` the codex `input_tokens` cell instead of reading it as a number        | `readers/codex.rs`  | totality: `panic in codex/archived (14 runs)`, 147 of 15,905 runs   |

The `codebuddy` reader shares the claude parser, so both claude breaks also fail `codebuddy/basic`.

## Detection

Four tests live in `tests/detect.rs`; the marker table and the two detector bridges live in `src/detect.rs`, and `ts-detect.mjs` runs `detectHarnesses` from `src/detect.ts` over a batch of homes in one Node process.
The Rust side is `skopli_wire::read::detect`, the entry the facades call, which marks a harness supported when its reader yields an event.

`layouts_detect_as_the_marker_table_says` enumerates home layouts over the 80 markers in `MARKERS`, each a directory or file `src/detect.ts` probes under the home with an empty environment and the home as working directory, tagged with the harness it speaks for or with the unsupported product it names.
Every root the detector spells relative to the home is a row, whichever operating system it serves: the VS Code `globalStorage` task root of each extension under `.config/Code`, `Library/Application Support/Code`, `.vscode-server/data` and `AppData/Roaming/Code`, the kiro database under `.kiro`, `.local/share/amazon-q` and `Library/Application Support/amazon-q`, and the Windows `AppData` roots of Cursor and Crush next to their Linux ones.
The layouts are the empty home, every single marker, every pair of markers and all markers at once: 3,242 homes.
The TypeScript detector is run twice over all of them and must return the same answer (a pure function of the layout), and each answer must be exactly the harnesses and products the present markers speak for.
The single-marker layouts establish that no two harnesses claim the same marker; `pi` (`.pi/agent/sessions`) and `kimchi` (`.config/kimchi/harness/sessions`) are distinct markers, which pins the decision that kimchi does not claim pi's session root.
Marker files are one empty line, so a reader opening one meets a real, empty record stream.
The Rust detector runs over the same homes and must claim no harness from any of them, since an empty marker holds no record; so on every enumerated layout the Rust verdict is a subset of the TypeScript one.

`rust_and_typescript_agree_on_golden_homes` copies every golden case to the default root its reader resolves with no environment override (`DEFAULT_ROOTS`, read off each reader's root resolver), one home per case plus one home holding all of them, and requires both detectors to report exactly that case's harness alone and exactly the set of all harnesses together.
Each home whose Linux root has a sibling on another operating system (`SIBLING_ROOTS`: the three other VS Code `globalStorage` roots, the macOS roots of CherryStudio, CommandCode, goose, kiro and Zed, the Block goose root and the Linux amazon-q root of kiro) is copied again with the tree moved to that sibling, 16 more homes over 62 in total, and must detect the same way; a sibling no golden home holds fails the check so the list cannot drift.
This is the agreement check between the two detectors: the TypeScript probe sees a marker where the Rust reader yields an event, for every harness and every root, and neither sees anything else.

`recorded_homes_probe_every_harness_that_yields_events` replays recorded homes read-only when `SKOPLI_VERIFY_HOMES` names a directory of `<harness>/<endpoint>/` homes.
Both detectors run on each; every harness the Rust pass yields events for must also be probed by TypeScript, and the per-home table (TypeScript verdict, Rust verdict, event count per harness, unsupported products) is printed and written to `SKOPLI_VERIFY_HOMES_OUT` when set.
The check is skipped with a note when the variable is unset, so the default run needs no recordings; when it is set to a path that is missing or holds no `<harness>/<endpoint>/` home the check fails naming the path, so a mistyped root cannot pass with an empty table.
`empty_recordings_root_fails_the_replay` pins that on a missing path and on a root with a harness directory but no endpoint.

```sh
SKOPLI_VERIFY_HOMES=<recordings dir> SKOPLI_VERIFY_HOMES_OUT=<table.md> \
  cargo test --release --manifest-path .agents/skills/verify-skopli-core/Cargo.toml --test detect -- --nocapture
```

### Bounds

- Markers present or absent, never partially valid: a present marker is an empty directory or a one-line file.
- At most two markers per layout plus the all-markers layout; three-marker layouts are not enumerated because detection is a disjunction over independent probes, and the pair layouts already catch a probe that depends on another marker.
- Empty environment and the home as working directory; the environment overrides and workspace-relative roots `src/detect.ts` also probes are not enumerated.
- Every default root `src/detect.ts` spells relative to the home, on Linux, macOS, VS Code Server and Windows alike.

Not covered: environment overrides (so the `APPDATA` and `LOCALAPPDATA` roots and the `XDG_*` roots are not probed), roots under a working directory other than the home (the `trajectories` marker is probed under the working directory, which the bare home also is), markers whose contents must parse for TypeScript to claim them, and a Rust claim on the marker layouts (its verdict needs a readable record, which an empty marker is not, so the layouts only establish that Rust claims nothing there; the golden-homes check, with its relocated siblings, covers the positive agreement).

### Assumptions

- `MARKERS` is a faithful reading of `src/detect.ts` for a bare home.
  A probe added to `src/detect.ts` without a row here passes the enumeration unnoticed, so the table must change in the same commit.
- The recordings a replay uses were produced by the harness itself; a home the Rust pass yields no events for is a reader finding or an unproductive recording, and the table says which by listing the TypeScript verdict alongside.

### Broken variants

Changing the gemini probe in `src/detect.ts` from `tmp` to `sessions` fails both checks: `70 of 2417 layouts disagree with the marker table` and `gemini/basic: typescript [], rust ["gemini"]` (run when the table had 69 markers).
Making `skopli_wire::read::detect` mark a harness supported when its reader raised a warning, not only when it yielded an event, fails the layout check: `820 of 2417 layouts disagree with the marker table`, starting with `[".local/share/opencode/opencode.db"]: rust claims ["opencode"] from empty markers`.
Changing the macOS VS Code root in `src/detect.ts` from `Application Support/Code` to `Application Support/Other` fails the layout check: `227 of 3242 layouts disagree with the marker table`, starting with the three single-marker macOS task roots of roo, cline and kilocode expected and not detected.
The same change to the macOS root in `crates/skopli-core/src/readers/vscode_ext.rs` fails the golden-homes check on the three relocated homes: `cline/ext-basic at Library/Application Support/Code/User/globalStorage: typescript ["cline"], rust [], want ["cline"]`, and the same for kilocode and roo.
Each variant was applied to the working tree, run, and reverted.

## When the verified code changes on purpose

- A new reader: add its golden case; it is picked up by every reader check and, if it has a default root, add the root to `DEFAULT_ROOTS` and its probe to `MARKERS`.
- A reader starts reading a cell it ignored: remove its `IGNORED_BY_DESIGN` row; the check fails until you do.
- A reader fixes a silent drop: remove its `KNOWN_SILENT_DROPS` row; the check fails until you do.
- A reader reads a new token cell name: add the name to `TokenClass::of`; until then the cell is not a token cell and is not checked.
- A reader starts deriving a counter from a new class, or a golden case gains a reachable token cell for a reader in `NO_MAPPING_COVERAGE`: update `TokenClass::derived` or remove the reader from the list; the check fails until you do.
- A new or moved probe in `src/detect.ts`: add or edit its `MARKERS` row with the harness it speaks for; a new unsupported product also needs a name in `unsupported_name`; a new root for data a golden home already holds at another root also needs a `SIBLING_ROOTS` pair.
- A change to the pi and kimchi ownership: both markers are rows in `MARKERS`, so the shared root would fail the single-marker layouts until the table says who owns it.
- A new rollup dimension: add it to the `by` list in `cases()` and a matching arm to `expected_key`.
- A new counter on `TokenCounts` or `Rollup`: add it to `Pattern`, `event()`, the `pairs` table in `check_conservation` and the parity JSON if it is optional.
- A change to the block rule or the default width: update `block_spec` and `golden/rollup-block/README.md` in the same change, in that order of authority.
- A change to the timestamp grammar: add the new accepted and rejected forms to `INSTANTS`, each with its epoch milliseconds, hour floor and day keys worked out by hand.
- A new day zone: add a column to `Instant` and an arm to `expected_key`; a change to the unknown-zone fallback changes the `Not/A_Zone` arm instead.
- A non-UTC block zone: `block_spec` floors in UTC and `Instant.hour` holds the UTC hour, so add a per-zone hour column and pass the zone into `block_spec` before adding the zone to `cases()`; until then such a case fails as a modeling error, not a finding.
- A change to the clamp, the finite-cost rule or the absent-calls default: update the matching line in `check_conservation` and say in the commit which promise changed.

Keep the domain small enough that a run stays under a minute; widen a single dimension rather than all of them.

## Limits

The exhaustive checks cover every input in their stated domains and say nothing about inputs outside them; only the Kani harnesses in `src/kani_proofs.rs` speak for all inputs, and only for the kernels they name.
The reader checks reach only the record shapes the golden corpus holds, and the detection checks only the bare-home probes.
Nothing here checks pricing, the wire format beyond `read_usage` and `detect`, or any facade other than TypeScript.

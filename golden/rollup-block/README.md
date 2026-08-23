# Billing-block rollup conformance fixtures

Shared gold for `rollup(by: "block", blockMs, tz)`: events aggregate into
session-window blocks that approximate a provider quota window. A block opens at
the first event's timestamp floored to the hour in the requested zone and spans
`blockMs`; events in `[start, start + blockMs)` join it, and the first event past
that opens a new block. Each row's key is the ISO-8601 instant of its block
start. This fixture pins that windowing so any consumer that anchors, splits, or
floors a block wrong fails against shared gold. The Rust core and every language
facade (which reaches the same core over the C ABI) consume it the same way.

## `cases.json`

An array of cases, each:

- `name`: a stable case identifier.
- `tz`: the IANA zone passed as the rollup `tz` option (drives the hour floor).
- `blockMs`: the block width in milliseconds passed as the rollup `blockMs`
  option (`18000000` is the five-hour default). The field is always present in
  the fixture; a zero or negative value is a policy input, not an omission, and
  falls back to the five-hour default. Omitting `blockMs` entirely (using the
  default) is tested per suite outside this fixture, since the shared schema
  requires the field. Keeping the field required is the cleaner choice: every
  consumer reads a plain integer with no null handling.
- `timestamps`: the ISO-8601 timestamps of the events to roll up. A consumer
  synthesizes one usage event per timestamp and rolls them up with
  `{ by: "block", tz, blockMs }`. Order is irrelevant; the rollup sorts events by
  timestamp before windowing.
- `expected`: the resulting blocks in ascending block-start order, each with its
  `key` (the ISO-8601 block-start instant) and `events` count.

## The cases

- `anchors-to-the-hour`: the first event at 09:17 anchors a block to 09:00; a
  later event within five hours joins it.
- `splits-after-window`: an event past `start + blockMs` opens a new block
  anchored to its own hour.
- `boundary-event-opens-next-block`: an event exactly at `start + blockMs` is
  outside the half-open window and opens the next block.
- `idle-gap-larger-than-window`: a gap far beyond one block opens a fresh block
  anchored to the later event's hour.
- `custom-one-hour-width`: a one-hour `blockMs` splits two events an hour apart
  into separate blocks.
- `unsorted-input-is-ordered`: events supplied out of order are sorted by
  timestamp before windowing.
- `kolkata-floors-local-hour`: `Asia/Kolkata` (UTC+5:30) floors the local
  wall-clock hour, so 09:17Z (14:47 local) anchors to 14:00 local = 08:30Z.
- `fall-back-first-occurrence-anchors-earlier-hour`: during the
  `America/New_York` fall-back, 05:30Z is the first `01:30` (EDT). Flooring
  subtracts the local minute/second from the instant, preserving the occurrence,
  so it anchors to 05:00Z, not the repeated hour's earlier occurrence.
- `fall-back-second-occurrence-anchors-later-hour`: 06:30Z is the second `01:30`
  (EST) of the same fall-back; occurrence-preserving flooring anchors it to
  06:00Z. The two occurrences resolve to different instants.
- `fall-back-both-occurrences-separate-blocks`: both repeated-hour events with a
  one-hour width open separate blocks (05:00Z and 06:00Z), pinning that the
  reference and core agree on repeated-hour flooring.
- `spring-forward-floors-local-hour`: across the `America/New_York`
  spring-forward gap, 06:30Z (01:30 EST) and 07:30Z (03:30 EDT) floor to 06:00Z
  and 07:00Z; with a one-hour width they open separate blocks.
- `three-consecutive-gaps`: four events each more than one one-hour block apart
  open four consecutive blocks, exercising the re-anchor branch more than once.
- `zero-block-ms-falls-back-to-default`: a `blockMs` of `0` falls back to the
  five-hour default, so two events within five hours share one block. This pins
  the non-positive-width policy.
- `negative-block-ms-falls-back-to-default`: a negative `blockMs` also falls back
  to the five-hour default.
- `non-canonical-slash-timestamp-is-invalid`: a `2026/01/01 09:30:00` timestamp
  is outside the shared ECMA-262 Date Time String grammar, so both the reference
  and the core route it to the `invalid-date` bucket rather than letting host
  `Date` leniency accept it on one side only.
- `offsetless-datetime-is-utc-on-both-sides`: an offsetless `YYYY-MM-DDTHH:mm:ss`
  datetime is treated as UTC by the shared grammar on both sides, so it buckets
  identically regardless of host timezone (host `Date` would read it as local
  time and diverge on a non-UTC host).

## Timestamp grammar

Both the reference (`src/rollup.ts`) and the core (`crates/skopli-core`) parse
and sort timestamps at the rollup boundary with the same strict ECMA-262 Date
Time String Format grammar (`YYYY-MM-DD` optionally followed by
`THH:mm(:ss(.sss)?)?` and `Z` or `±HH:mm`; an offsetless date-time is UTC).
Anything outside that grammar sorts last and shares the single `invalid-date`
block. This keeps a caller-supplied event list bucketed identically on both
sides, since `rollup` is a public function and does not require reader-canonical
input.

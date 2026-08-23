# Timezone day-bucketing conformance fixtures

Shared gold for `rollup(by: "day", tz)`: the calendar date an event's timestamp
maps to depends on the requested IANA zone, DST transitions included. This
fixture pins that mapping so any consumer that buckets a non-UTC day wrong fails
against shared gold. The Rust core and every language facade (which reaches the
same core over the C ABI) consume it the same way.

## `cases.json`

An array of cases, each:

- `name`: a stable case identifier.
- `tz`: the IANA zone passed as the rollup `tz` option.
- `timestamps`: the ISO-8601 timestamps of the events to roll up by day. A
  consumer synthesizes one usage event per timestamp (the other event fields are
  irrelevant to day bucketing) and rolls them up with `{ by: "day", tz }`.
- `expected`: the resulting day buckets, one entry per calendar date, each with
  its `key` (`YYYY-MM-DD`) and `events` count. Buckets are sorted ascending by
  `key`, matching the rollup output order.

## The cases

- `utc-near-midnight`: two events straddling UTC midnight land in two UTC days.
- `tokyo-rolls-forward`: the same window in `Asia/Tokyo` (UTC+9) both fall on the
  next local day.
- `los-angeles-rolls-back`: two events in `America/Los_Angeles` (UTC-7 in
  August) both fall on the previous local day.
- `kolkata-half-hour-offset`: `Asia/Kolkata` (UTC+5:30) crosses local midnight on
  a half-hour offset.
- `new-york-dst-spring-forward`: two events either side of the 2026-03-08
  spring-forward transition in `America/New_York` share the same local day.
- `new-york-dst-fall-back`: the two occurrences of the repeated 01:30 hour on
  2026-11-01 in `America/New_York` (`05:30Z` is 01:30 EDT, the first fold
  instant; `06:30Z` is 01:30 EST, the repeated hour) both bucket to the same
  local day, exercising both sides of the fall-back fold.
- `unknown-zone-falls-back-to-utc`: an unknown zone degrades to the UTC calendar
  date rather than aborting the rollup.

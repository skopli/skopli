# Cache timestamp boundary conformance fixtures

Shared gold for the cache `fetchedAt` wire contract: one ISO-8601 stamp string
read and written by seven different date libraries. A stamp written by one
facade can be read by any other (or by a human editing the shared cache file),
so the READ side must tolerate every shape the contract admits and agree, to the
millisecond, on the epoch it maps to. The WRITE side is narrower: every facade
emits an integral-millisecond, `Z`-suffixed, 24-character stamp. This fixture
pins both so any consumer that drifts fails against shared gold.

## `cases.json`

An object with two arrays:

### `read`

Each entry maps a stamp string to the epoch it must parse to:

- `name`: a stable case identifier.
- `stamp`: the `fetchedAt` string as it appears in a cache file.
- `epochMs`: the expected result. An integer number of milliseconds since the
  Unix epoch when the stamp is valid, or `null` when the stamp is malformed. A
  consumer parses `stamp` through its cache-read path; a valid stamp yields
  `epochMs`, a `null` case is rejected (treated as a stale or missing cache, not
  a crash).

### `write`

An array of epoch-millisecond integers. A consumer formats each through its
cache-write path and asserts the result is a 24-character, `Z`-suffixed,
integral-millisecond stamp of the form `YYYY-MM-DDTHH:MM:SS.mmmZ` that reads
back to the same epoch. Facades only ever write stamps they generated
themselves, so the write contract is integral-ms shape rather than adversarial
input tolerance.

## The read cases

- `integral-ms`: the canonical shape a facade writes, three fractional digits
  and a `Z` suffix.
- `zero-ms-short-form`: no fractional part, still valid and equal to the same
  instant with `.000`.
- `microseconds-truncated`: six fractional digits truncate to integral
  milliseconds (`.123456` maps to `123` ms), the sane consensus across the
  facades rather than rounding to `124`.
- `pre-epoch`: one second before the Unix epoch parses to `-1000` ms; a
  negative epoch is a valid instant, not an error.
- `offset-equals-midnight-z`: a `+01:00` offset stamp names the same instant as
  midnight `Z`, so it maps to the same epoch as `integral-ms`.
- `garbage`: a string that is not a timestamp is rejected (`null`), invalidating
  the cache entry instead of crashing.

## The write cases

- `1785542400000`: the `integral-ms` read instant, round-tripping to
  `2026-08-01T00:00:00.000Z`.
- `0`: the Unix epoch, `1970-01-01T00:00:00.000Z`.
- `1000000000000`: a mid-range instant, `2001-09-09T01:46:40.000Z`.

//! Epoch-millis -> ISO-8601 string, replicating V8 `Date.prototype.toISOString()`.
//!
//! Readers that CONSTRUCT timestamps from epoch numbers (the SQLite-backed
//! readers) must format them exactly as V8 does, since the golden files were
//! produced by the real TS reader running on V8. claude and the other JSONL
//! readers pass timestamp strings through verbatim and never call this.
//!
//! Grammar (per ECMA-262 `Date.prototype.toISOString` / `DateTimeString`):
//! `YYYY-MM-DDTHH:mm:ss.sssZ` - always 3 fractional (millisecond) digits, a
//! literal `Z`, and a 4-digit zero-padded year for years `0000..=9999`.
//! Outside that range V8 emits the expanded form `±YYYYYY` (a sign plus six
//! digits): `+` for years `> 9999`, `-` for years `< 0`.
//!
//! Range: V8's time-value range is `±8.64e15` ms (100_000_000 days either side
//! of the epoch). `new Date(ms).toISOString()` throws `RangeError` outside it;
//! here we return `None` (the TS-side `RangeError` is a caller concern).
//!
//! Civil-date math is Howard Hinnant's public-domain `civil_from_days`
//! algorithm (<http://howardhinnant.github.io/date_algorithms.html>),
//! re-derived below - no `chrono`/`time` dependency.

const MS_PER_SEC: i64 = 1_000;
const MS_PER_MIN: i64 = 60 * MS_PER_SEC;
const MS_PER_HOUR: i64 = 60 * MS_PER_MIN;
const MS_PER_DAY: i64 = 24 * MS_PER_HOUR;

/// V8/ECMA-262 maximum absolute time value, in milliseconds (`8.64e15`).
const MAX_TIME_MS: i64 = 8_640_000_000_000_000;

/// Format `epoch_ms` as V8's `Date.prototype.toISOString()` would.
///
/// Returns `None` when `epoch_ms` is outside V8's `±8.64e15` ms range, which
/// is exactly where `toISOString()` throws `RangeError` in JS. Never panics.
pub fn to_iso_string(epoch_ms: i64) -> Option<String> {
    if !(-MAX_TIME_MS..=MAX_TIME_MS).contains(&epoch_ms) {
        return None;
    }

    // Split into whole days and the positive millis-of-day remainder. Floor
    // division so that pre-1970 (negative) epochs step back a full day and
    // keep a non-negative time-of-day, matching V8's `Day`/`TimeWithinDay`.
    let days = epoch_ms.div_euclid(MS_PER_DAY);
    let ms_of_day = epoch_ms.rem_euclid(MS_PER_DAY);

    let (year, month, day) = civil_from_days(days);

    let hour = ms_of_day / MS_PER_HOUR;
    let minute = (ms_of_day % MS_PER_HOUR) / MS_PER_MIN;
    let second = (ms_of_day % MS_PER_MIN) / MS_PER_SEC;
    let milli = ms_of_day % MS_PER_SEC;

    let year_str = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else if year > 9999 {
        format!("+{year:06}")
    } else {
        // year < 0: sign then six digits of the magnitude.
        format!("-{:06}", -year)
    };

    Some(format!(
        "{year_str}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z"
    ))
}

/// Convert a count of days since 1970-01-01 to `(year, month, day)`, where
/// `month` and `day` are 1-based. Howard Hinnant's `civil_from_days`, valid for
/// the full range we admit. Works for negative `z` (pre-epoch) unchanged.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so leap-day handling is uniform.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11] (Mar=0)
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ground truth: each expected string is the exact output of
    //   node -e 'console.log(new Date(<ms>).toISOString())'
    // on node v24 (V8). The two out-of-range inputs throw `RangeError` in JS
    // and map to `None` here.
    //
    // ms                     expected
    // 1700000000123          2023-11-14T22:13:20.123Z
    // 0                      1970-01-01T00:00:00.000Z
    // -1                     1969-12-31T23:59:59.999Z
    // -1500                  1969-12-31T23:59:58.500Z
    // -123456789             1969-12-30T13:42:23.211Z
    // -62135596800000        0001-01-01T00:00:00.000Z   (year 1)
    // -62167219200000        0000-01-01T00:00:00.000Z   (year 0)
    // -62200000000000        -000002-12-17T14:13:20.000Z (expanded negative)
    // 253402300799999        9999-12-31T23:59:59.999Z   (last ms of 9999)
    // 253402300800000        +010000-01-01T00:00:00.000Z (first ms of 10000)
    // 8640000000000000       +275760-09-13T00:00:00.000Z (max range)
    // -8640000000000000      -271821-04-20T00:00:00.000Z (min range)
    // 8640000000000001       RangeError -> None
    // -8640000000000001      RangeError -> None

    fn iso(ms: i64) -> String {
        to_iso_string(ms).expect("in-range")
    }

    #[test]
    fn normal_date() {
        assert_eq!(iso(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
    }

    #[test]
    fn epoch_zero() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn negative_epoch_one_ms() {
        assert_eq!(iso(-1), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn negative_epoch_millis_remainder() {
        // -1500 ms -> floor to the prior day, positive 58.500s remainder.
        assert_eq!(iso(-1500), "1969-12-31T23:59:58.500Z");
    }

    #[test]
    fn negative_epoch_pre_1970() {
        assert_eq!(iso(-123_456_789), "1969-12-30T13:42:23.211Z");
    }

    #[test]
    fn year_one_boundary() {
        assert_eq!(iso(-62_135_596_800_000), "0001-01-01T00:00:00.000Z");
    }

    #[test]
    fn year_zero() {
        assert_eq!(iso(-62_167_219_200_000), "0000-01-01T00:00:00.000Z");
    }

    #[test]
    fn expanded_negative_year() {
        assert_eq!(iso(-62_200_000_000_000), "-000002-12-17T14:13:20.000Z");
    }

    #[test]
    fn year_9999_last_ms() {
        assert_eq!(iso(253_402_300_799_999), "9999-12-31T23:59:59.999Z");
    }

    #[test]
    fn year_10000_first_ms_expanded() {
        assert_eq!(iso(253_402_300_800_000), "+010000-01-01T00:00:00.000Z");
    }

    #[test]
    fn max_range_boundary() {
        assert_eq!(iso(MAX_TIME_MS), "+275760-09-13T00:00:00.000Z");
    }

    #[test]
    fn min_range_boundary() {
        assert_eq!(iso(-MAX_TIME_MS), "-271821-04-20T00:00:00.000Z");
    }

    #[test]
    fn over_max_range_is_none() {
        assert_eq!(to_iso_string(MAX_TIME_MS + 1), None);
    }

    #[test]
    fn under_min_range_is_none() {
        assert_eq!(to_iso_string(-MAX_TIME_MS - 1), None);
    }
}

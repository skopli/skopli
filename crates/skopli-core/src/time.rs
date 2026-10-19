//! The ECMA-262 date-time string grammar in both directions, replicating V8.
//!
//! [`to_iso_string`] is `Date.prototype.toISOString()`: always three
//! fractional digits, a literal `Z`, a four-digit year for `0000..=9999` and the
//! expanded `±YYYYYY` form outside it. Readers that construct timestamps from
//! epoch numbers must format them this way because the golden files were
//! produced by the TypeScript reader running on V8.
//!
//! [`date_parse_ms`] is `Date.parse` restricted to the Date Time String Format,
//! which is the only shape the facades exchange.
//!
//! V8's time value range is `±8.64e15` ms; `toISOString()` throws `RangeError`
//! outside it, which is `None` here.

const MS_PER_SEC: i64 = 1_000;
const MS_PER_MIN: i64 = 60 * MS_PER_SEC;
const MS_PER_HOUR: i64 = 60 * MS_PER_MIN;
const MS_PER_DAY: i64 = 24 * MS_PER_HOUR;

/// V8/ECMA-262 maximum absolute time value, in milliseconds (`8.64e15`).
const MAX_TIME_MS: i64 = 8_640_000_000_000_000;

/// Format `epoch_ms` as V8's `Date.prototype.toISOString()` would, or `None`
/// outside V8's `±8.64e15` ms range.
pub fn to_iso_string(epoch_ms: i64) -> Option<String> {
    if !(-MAX_TIME_MS..=MAX_TIME_MS).contains(&epoch_ms) {
        return None;
    }
    let (year, month, day) = civil_from_days(epoch_ms.div_euclid(MS_PER_DAY));
    let ms_of_day = epoch_ms.rem_euclid(MS_PER_DAY);
    let hour = ms_of_day / MS_PER_HOUR;
    let minute = ms_of_day % MS_PER_HOUR / MS_PER_MIN;
    let second = ms_of_day % MS_PER_MIN / MS_PER_SEC;
    let milli = ms_of_day % MS_PER_SEC;
    let year = match year {
        0..=9999 => format!("{year:04}"),
        10000.. => format!("+{year:06}"),
        _ => format!("-{:06}", -year),
    };
    Some(format!(
        "{year}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z"
    ))
}

/// Parse an ECMA-262 Date Time String Format timestamp to epoch millis:
/// `YYYY-MM-DD` (`T`HH:mm(:ss(.sss)?)?)? (Z | ±HH:mm)?
/// A date-only string is UTC midnight and a date-time without an offset is UTC.
/// Fractions longer than three digits are truncated, shorter ones zero-padded.
/// Anything else is `None`.
pub fn date_parse_ms(input: &str) -> Option<i64> {
    let s = input.as_bytes();
    let digits = |from: usize, len: usize| -> Option<i64> {
        let slice = s.get(from..from + len)?;
        slice.iter().all(u8::is_ascii_digit).then(|| {
            slice
                .iter()
                .fold(0, |acc, b| acc * 10 + i64::from(b - b'0'))
        })
    };
    if s.len() < 10 || s[4] != b'-' || s[7] != b'-' {
        return None;
    }
    let (year, month, day) = (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (mut hour, mut minute, mut second, mut milli, mut offset_min) = (0, 0, 0, 0, 0);
    if s.len() > 10 {
        if !matches!(s[10], b'T' | b' ') || s.len() < 16 || s[13] != b':' {
            return None;
        }
        hour = digits(11, 2)?;
        minute = digits(14, 2)?;
        let mut idx = 16;
        if s.get(idx) == Some(&b':') {
            second = digits(17, 2)?;
            idx = 19;
            if s.get(idx) == Some(&b'.') {
                let frac_start = idx + 1;
                let frac_end = s[frac_start..]
                    .iter()
                    .position(|b| !b.is_ascii_digit())
                    .map_or(s.len(), |n| frac_start + n);
                if frac_end == frac_start {
                    return None;
                }
                milli = s[frac_start..frac_end]
                    .iter()
                    .chain(b"00")
                    .take(3)
                    .fold(0, |acc, b| acc * 10 + i64::from(b - b'0'));
                idx = frac_end;
            }
        }
        match s.get(idx) {
            None => {}
            Some(b'Z') if s.len() == idx + 1 => {}
            Some(sign @ (b'+' | b'-')) if s.len() >= idx + 6 && s[idx + 3] == b':' => {
                let magnitude = digits(idx + 1, 2)? * 60 + digits(idx + 4, 2)?;
                offset_min = if *sign == b'-' { -magnitude } else { magnitude };
            }
            Some(_) => return None,
        }
    }
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=59).contains(&second) {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    Some(
        days * MS_PER_DAY + hour * MS_PER_HOUR + minute * MS_PER_MIN + second * MS_PER_SEC + milli
            - offset_min * MS_PER_MIN,
    )
}

/// Days since 1970-01-01 to a 1-based proleptic Gregorian `(year, month,
/// day)`. Counts from 0000-03-01 so a leap day is the last day of the year.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

/// The inverse of [`civil_from_days`].
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso(ms: i64) -> String {
        to_iso_string(ms).expect("in-range")
    }

    // Each expected string is the exact output of `new Date(ms).toISOString()`
    // on V8. The two out-of-range inputs throw `RangeError` there.
    #[test]
    fn formats_like_v8() {
        assert_eq!(iso(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(iso(-1500), "1969-12-31T23:59:58.500Z");
        assert_eq!(iso(-123_456_789), "1969-12-30T13:42:23.211Z");
        assert_eq!(iso(-62_135_596_800_000), "0001-01-01T00:00:00.000Z");
        assert_eq!(iso(-62_167_219_200_000), "0000-01-01T00:00:00.000Z");
        assert_eq!(iso(-62_200_000_000_000), "-000002-12-17T14:13:20.000Z");
        assert_eq!(iso(253_402_300_799_999), "9999-12-31T23:59:59.999Z");
        assert_eq!(iso(253_402_300_800_000), "+010000-01-01T00:00:00.000Z");
        assert_eq!(iso(MAX_TIME_MS), "+275760-09-13T00:00:00.000Z");
        assert_eq!(iso(-MAX_TIME_MS), "-271821-04-20T00:00:00.000Z");
        assert_eq!(to_iso_string(MAX_TIME_MS + 1), None);
        assert_eq!(to_iso_string(-MAX_TIME_MS - 1), None);
    }

    #[test]
    fn parses_every_admitted_shape() {
        assert_eq!(date_parse_ms("1970-01-01"), Some(0));
        assert_eq!(date_parse_ms("1970-01-01T00:01"), Some(60_000));
        assert_eq!(date_parse_ms("1970-01-01 00:00:01"), Some(1000));
        assert_eq!(date_parse_ms("1970-01-01T00:00:00.5Z"), Some(500));
        assert_eq!(date_parse_ms("1970-01-01T00:00:00.123456Z"), Some(123));
        assert_eq!(date_parse_ms("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(date_parse_ms("1969-12-31T23:59:59.000Z"), Some(-1000));
        assert_eq!(
            date_parse_ms("2023-11-14T22:13:20.123Z"),
            Some(1_700_000_000_123)
        );
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "",
            "not-a-date",
            "2026/01/01",
            "2026-13-01",
            "2026-01-32",
            "2026-01-01T24:00",
            "2026-01-01T00:00:00.Z",
            "2026-01-01T00:00:00+0100",
            "2026-01-01T00:00:00ZZ",
            "2026-01-01X00:00",
        ] {
            assert_eq!(date_parse_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn civil_round_trips_across_eras() {
        for days in [
            -1_000_000, -719_468, -1, 0, 1, 59, 60, 365, 20_000, 2_932_896,
        ] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }
}

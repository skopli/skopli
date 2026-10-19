use std::fs;
use std::path::Path;

use serde_json::Value;
use walkdir::WalkDir;

pub use crate::time::date_parse_ms;
use crate::time::to_iso_string;
use crate::types::UsageEvent;

/// A warning surfaced while reading; the caller decides how to present it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderWarning {
    pub message: String,
}

/// Result of running a reader: the parsed events, the paths or locations that
/// were skipped, and the warnings emitted.
#[derive(Debug, Clone, Default)]
pub struct ReaderResult {
    pub events: Vec<UsageEvent>,
    pub skipped: Vec<String>,
    pub warnings: Vec<ReaderWarning>,
}

/// Recursively list files under `dir` whose file name matches `predicate`,
/// sorted by the full path string with native separators. Unreadable
/// directories are ignored.
pub fn list_files(dir: &Path, predicate: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.file_name().to_str().map(&predicate).unwrap_or(false))
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// True when `path` is an existing directory.
pub fn dir_exists(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

/// True when `value` is a JSON object (not array, not null).
pub fn is_record(value: &Value) -> bool {
    value.is_object()
}

/// The finite number value, or `None` for NaN, infinities and non-numbers.
pub fn finite_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}

/// The string value, or `None`.
pub fn as_string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// A JSON scalar as text: a string as is, a number or bool stringified, else
/// empty. Used for row ids that a store may hold as any scalar type.
pub fn scalar_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// A number as JSON text: integers without a fractional part, anything else in
/// Rust's shortest round-trip form. Integral values at or beyond 2^63 saturate
/// to `i64::MAX`; token counts and ledger ids stay far below that.
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A finite number, else 0.
pub fn as_number(value: &Value) -> f64 {
    finite_number(value).unwrap_or(0.0)
}

/// Read and parse a JSON file; `None` when the file is unreadable or malformed.
pub fn read_json(path: &str) -> Option<Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&content).ok()
}

/// The epoch unit for [`epoch_to_iso_unit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochUnit {
    /// The number is epoch milliseconds.
    Ms,
    /// The number is epoch seconds.
    Sec,
    /// `>= 1e12` -> millis, smaller positive -> seconds.
    Auto,
}

/// Parse a value as an epoch (auto unit) and format it as an ISO string, or
/// `None`. Convenience for `epoch_to_iso_unit(value, EpochUnit::Auto)`.
pub fn epoch_to_iso(value: &Value) -> Option<String> {
    epoch_to_iso_unit(value, EpochUnit::Auto)
}

/// Parse a value as an epoch and format it as an ISO string, or `None`.
/// Non-finite or `<= 0` -> `None`; the `unit` selects the seconds/millis scale
/// (`Auto`: values `>= 1e12` are millis, smaller positive values are seconds).
/// Epochs outside the ECMA-262 time value range map to `None`.
pub fn epoch_to_iso_unit(value: &Value, unit: EpochUnit) -> Option<String> {
    let numeric = finite_number(value)?;
    if numeric <= 0.0 {
        return None;
    }
    let ms = match unit {
        EpochUnit::Ms => numeric,
        EpochUnit::Sec => numeric * 1000.0,
        EpochUnit::Auto => {
            if numeric >= 1e12 {
                numeric
            } else {
                numeric * 1000.0
            }
        }
    };
    to_iso_string(ms.trunc() as i64)
}

/// Resolve a timestamp that may arrive as an ISO/RFC-3339 string, a numeric
/// string, or a number, to an ISO string:
/// - a number -> `epoch_to_iso` (auto unit);
/// - a numeric string (`^-?\d+(\.\d+)?$`) -> its numeric value via auto epoch;
/// - any other string -> parsed as an ISO date-time (a bare datetime without a
///   timezone is read as UTC), else `None`;
/// - a non-string, non-number, or empty/whitespace string -> `None`.
pub fn parse_timestamp_to_iso(value: &Value) -> Option<String> {
    if value.is_number() {
        return epoch_to_iso(value);
    }
    let raw = value.as_str()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_numeric_literal(trimmed) {
        let numeric: f64 = trimmed.parse().ok()?;
        if !numeric.is_finite() {
            return None;
        }
        return epoch_to_iso(&Value::from(numeric));
    }
    reparse_iso(trimmed)
}

/// True when `s` is an integer or decimal literal with an optional leading
/// minus (`^-?\d+(\.\d+)?$`).
fn is_numeric_literal(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'-') {
        i = 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == int_start {
        return false;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            return false;
        }
    }
    i == bytes.len()
}

/// Re-serialize an ECMA-262 Date Time String Format timestamp as
/// `YYYY-MM-DDTHH:mm:ss.sssZ`, or `None` when [`date_parse_ms`] rejects it.
pub fn reparse_iso(input: &str) -> Option<String> {
    to_iso_string(date_parse_ms(input)?)
}

/// The file's mtime as an ISO string, or the epoch when unavailable.
pub fn file_mtime_iso(file: &str) -> String {
    let epoch_ms = fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    to_iso_string(epoch_ms).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_owned())
}

/// The last path component of a slash- or backslash-separated path.
pub fn basename(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit('/')
        .next()
        .unwrap_or(&normalized)
        .to_owned()
}

/// The file name without its final extension; a leading dot is not an extension.
pub fn file_stem_name(path: &str) -> String {
    let name = basename(path);
    match name.rfind('.') {
        Some(0) | None => name,
        Some(idx) => name[..idx].to_owned(),
    }
}

/// The directory part of a slash- or backslash-separated path, without the
/// trailing separator. Empty when the path has no directory part.
pub fn dirname(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rfind('/') {
        Some(i) => normalized[..i].to_owned(),
        None => String::new(),
    }
}

/// A token cell the producer always writes. `None` (malformed) when absent,
/// `null`, or not a non-negative finite number.
pub fn required_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// A token cell the producer may omit. Absent or `null` is `0`; a populated
/// cell that is not a non-negative finite number is malformed (`None`).
pub fn optional_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// The parent directory's name.
pub fn parent_dir_name(path: &str) -> String {
    basename(&dirname(path))
}

/// A dedup-key function for `scan_jsonl`: maps a parsed value to an optional
/// key; `None` means "never dedup this value".
pub type DedupKey<'a, T> = &'a dyn Fn(&T) -> Option<String>;

/// Options for a JSONL scan across many files.
pub struct ScanJsonl<'a, T> {
    pub files: &'a [String],
    pub parse: &'a mut dyn FnMut(&JsonlLine, &str) -> Option<T>,
    pub dedup_key: Option<DedupKey<'a, T>>,
}

/// Scan JSONL files, parsing each line and optionally deduping. Malformed lines
/// / unreadable files are recorded through `read_jsonl_lines` into `skipped` +
/// `warnings`.
pub fn scan_jsonl<T>(
    opts: ScanJsonl<'_, T>,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<T> {
    let mut values = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for file in opts.files {
        let lines = match read_jsonl_lines(file, skipped, warnings) {
            Some(l) => l,
            None => continue,
        };
        for line in &lines {
            let value = match (opts.parse)(line, file) {
                Some(v) => v,
                None => continue,
            };
            if let Some(dedup) = opts.dedup_key
                && let Some(key) = dedup(&value)
            {
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);
            }
            values.push(value);
        }
    }
    values
}

/// Normalize a workspace path string.
pub fn normalize_workspace(value: &Value) -> Option<String> {
    let raw = value.as_str()?;
    let forward = raw.trim().replace('\\', "/");
    if forward.is_empty() {
        return None;
    }
    let unc = forward.starts_with("//") && !forward.starts_with("///");
    let prefix = if unc {
        "//"
    } else if forward.starts_with('/') {
        "/"
    } else {
        ""
    };
    let after_prefix = &forward[prefix.len()..];
    let after_leading = after_prefix.trim_start_matches('/');
    let body = collapse_slashes(after_leading);

    let drive_only = is_drive_only(&body);
    let trimmed = if drive_only {
        body.strip_suffix('/').unwrap_or(&body).to_owned()
    } else {
        body.trim_end_matches('/').to_owned()
    };
    let path = if drive_only {
        format!("{prefix}{trimmed}/")
    } else {
        format!("{prefix}{trimmed}")
    };
    let normalized = if unc || is_drive_prefixed(&body) {
        path.to_lowercase()
    } else {
        path
    };
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn collapse_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
            }
            prev_slash = true;
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    out
}

/// True for a bare drive (`C:` or `C:/`).
fn is_drive_only(body: &str) -> bool {
    let b = body.as_bytes();
    match b.len() {
        2 => b[0].is_ascii_alphabetic() && b[1] == b':',
        3 => b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/',
        _ => false,
    }
}

/// True when `body` starts with a drive letter and colon.
fn is_drive_prefixed(body: &str) -> bool {
    let b = body.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// A single parsed JSONL line: the 0-based line index and its JSON value.
pub struct JsonlLine {
    pub index: usize,
    pub value: Value,
}

/// Read a JSONL file into parsed lines:
/// - splits on "\n", trims each line, skips empty lines;
/// - malformed JSON lines are warned + pushed to `skipped` as `file:index+1`;
/// - an unreadable file warns + pushes the file path to `skipped`, returns None.
pub fn read_jsonl_lines(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Option<Vec<JsonlLine>> {
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            warnings.push(ReaderWarning {
                message: format!("skipping unreadable file {file}\n"),
            });
            skipped.push(file.to_owned());
            return None;
        }
    };
    let mut out = Vec::new();
    for (i, raw) in content.split('\n').enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(value) => out.push(JsonlLine { index: i, value }),
            Err(_) => {
                let loc = format!("{file}:{}", i + 1);
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed line {loc}\n"),
                });
                skipped.push(loc);
            }
        }
    }
    Some(out)
}

/// A fixed fake home for root-resolution tests.
#[cfg(test)]
pub(crate) fn test_home() -> std::path::PathBuf {
    std::path::PathBuf::from("/home/u")
}

/// A fresh per-test scratch directory under the system temp dir.
#[cfg(test)]
pub(crate) fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!(
        "skopli-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&base).unwrap();
    base
}

#[cfg(test)]
mod tests {
    use super::reparse_iso;

    #[test]
    fn fractional_seconds_are_truncated_to_milliseconds() {
        let cases = [
            ("2026-10-18T18:30:27.9995Z", "2026-10-18T18:30:27.999Z"),
            ("2026-10-18T18:30:27.1235Z", "2026-10-18T18:30:27.123Z"),
            ("2026-10-18T18:30:27.123456789Z", "2026-10-18T18:30:27.123Z"),
            ("2026-10-18T18:30:27.5Z", "2026-10-18T18:30:27.500Z"),
            ("2026-10-18T18:30:27.12+01:00", "2026-10-18T17:30:27.120Z"),
        ];
        for (input, expected) in cases {
            assert_eq!(reparse_iso(input).as_deref(), Some(expected), "{input}");
        }
        assert_eq!(reparse_iso("2026-10-18T18:30:27.Z"), None);
    }
}

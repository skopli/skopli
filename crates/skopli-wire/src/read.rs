//! The multi-reader `read_usage` envelope and harness detection shared by the
//! direct-core bindings that own the whole read+filter loop (`skopli-py`,
//! `skopli-ruby`). `skopli-node` instead exposes a per-harness raw
//! gather (`read_harness`) and runs the filter loop in its TS facade, so it does
//! NOT use this module.
//!
//! Seam split: `{home, env}` + since/until/tz travel as
//! data in the options JSON; a date-only bound resolves to midnight of that day
//! in the requested `tz` (via the bundled tzdb, DST-aware), so date filtering
//! and day rollups agree on where a calendar day begins. An omitted `tz`
//! resolves the system timezone (matching the TS default); `"UTC"` and any
//! unknown zone resolve to UTC midnight. Pricing
//! source loading / fetch / cache live in the language facade, which hands
//! pre-fetched catalog JSON to `create_pricing`; nothing here touches the
//! network.

use serde_json::{Value, json};
use skopli_core::readers::reader::ReaderContext;
use skopli_core::readers::registered_readers;
use skopli_core::types::UsageEvent;

/// Options accepted by `read_usage`, parsed from the options JSON.
pub struct ReadOptions {
    pub ctx: ReaderContext,
    pub harnesses: Option<Vec<String>>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub exclude_subagents: bool,
}

/// Parse `read_usage` options from the JSON value. Bad `since`/`until` yield an
/// `Err(message)` that the caller turns into an `InvalidArgumentError`.
pub fn parse_read_options(opts: &Value) -> Result<ReadOptions, String> {
    let ctx = crate::parse::context_from_options(opts);

    let harnesses = opts.get("harnesses").and_then(Value::as_array).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    });

    let tz = opts.get("tz").and_then(Value::as_str);
    let since_ms = parse_bound(opts.get("since"), tz, false)?;
    let until_ms = parse_bound(opts.get("until"), tz, true)?;
    let exclude_subagents = opts.get("subagents").and_then(Value::as_str) == Some("exclude");

    Ok(ReadOptions {
        ctx,
        harnesses,
        since_ms,
        until_ms,
        exclude_subagents,
    })
}

/// Resolve a since/until bound to epoch ms. Mirrors `toTime` in read.ts: a
/// date-only `YYYY-MM-DD` string is midnight of that day in `tz` (DST-aware, via
/// the bundled tzdb; an omitted `tz` uses the system zone, `"UTC"`/unknown zone
/// -> UTC midnight), matching the zone the rollup day bucketing uses. Any other
/// string is parsed as ISO.
/// `exclusive_end` shifts a date-only `until` to midnight of the following day.
/// An unparseable value is an error.
fn parse_bound(
    value: Option<&Value>,
    tz: Option<&str>,
    exclusive_end: bool,
) -> Result<Option<i64>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let s = value
        .as_str()
        .ok_or_else(|| "since/until must be a date string".to_owned())?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if let Some((y, m, d)) = parse_date_only(trimmed) {
        let day_offset = if exclusive_end { 1 } else { 0 };
        return zoned_midnight(y, m, d, tz, day_offset)
            .ok_or_else(|| format!("invalid date: {s}"))
            .map(Some);
    }
    match skopli_core::readers::shared::date_parse_ms(trimmed) {
        Some(ms) => Ok(Some(ms)),
        None => Err(format!("invalid date: {s}")),
    }
}

/// Parse a bare `YYYY-MM-DD` (no time component) into `(year, month, day)`, or
/// `None` if it is not in that exact form.
fn parse_date_only(s: &str) -> Option<(i16, i8, i8)> {
    let bytes = s.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(i, &b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return None;
    }
    let year: i16 = s[0..4].parse().ok()?;
    let month: i8 = s[5..7].parse().ok()?;
    let day: i8 = s[8..10].parse().ok()?;
    Some((year, month, day))
}

/// Midnight of `year-month-day` (plus `day_offset` days) in `tz`, as epoch ms.
/// An omitted `tz` resolves the system IANA zone (matching the TS default of the
/// process system timezone; jiff falls back to UTC if the system zone is
/// undetectable). `"UTC"` and any unknown zone use UTC. An ambiguous wall-clock
/// midnight (a DST fold) takes its first occurrence and a midnight inside a DST
/// gap takes the first valid instant of the day, matching read.ts (jiff's
/// `Compatible` disambiguation). Returns `None` for a calendar-invalid date.
fn zoned_midnight(year: i16, month: i8, day: i8, tz: Option<&str>, day_offset: i64) -> Option<i64> {
    let zone = match tz {
        None => jiff::tz::TimeZone::system(),
        Some("UTC") => jiff::tz::TimeZone::UTC,
        Some(name) => jiff::tz::TimeZone::get(name).unwrap_or(jiff::tz::TimeZone::UTC),
    };
    let date = jiff::civil::Date::new(year, month, day).ok()?;
    let date = date.checked_add(jiff::Span::new().days(day_offset)).ok()?;
    let ts = zone.to_timestamp(date.at(0, 0, 0, 0)).ok()?;
    Some(ts.as_millisecond())
}

/// The `read_usage` result envelope: `{events, diagnostics, skipped}`. Events
/// are the union across the selected harnesses, in per-harness read order.
pub fn read_usage(options: &ReadOptions) -> Value {
    let selected: Option<std::collections::HashSet<&str>> = options
        .harnesses
        .as_ref()
        .map(|h| h.iter().map(String::as_str).collect());

    let mut events: Vec<UsageEvent> = Vec::new();
    let mut diagnostics: Vec<Value> = Vec::new();
    let mut skipped: serde_json::Map<String, Value> = serde_json::Map::new();

    for reader in registered_readers() {
        let harness = reader.harness_id();
        if let Some(sel) = &selected
            && !sel.contains(harness)
        {
            continue;
        }
        let result = reader.read(&options.ctx);

        for warning in &result.warnings {
            diagnostics.push(json!({
                "severity": "warning",
                "message": warning.message.trim_end_matches('\n'),
                "harness": harness,
            }));
        }
        if !result.skipped.is_empty() {
            skipped.insert(harness.to_owned(), json!(result.skipped));
        }

        for event in result.events {
            match skopli_core::readers::shared::date_parse_ms(&event.timestamp) {
                None => {
                    diagnostics.push(json!({
                        "severity": "warning",
                        "message": format!(
                            "dropping event {} with unusable timestamp {}",
                            event.message_id,
                            serde_json::to_string(&event.timestamp).unwrap_or_default()
                        ),
                        "harness": harness,
                    }));
                    continue;
                }
                Some(at) => {
                    if let Some(since) = options.since_ms
                        && at < since
                    {
                        continue;
                    }
                    if let Some(until) = options.until_ms
                        && at >= until
                    {
                        continue;
                    }
                    if options.exclude_subagents && event.subagent {
                        continue;
                    }
                    events.push(event);
                }
            }
        }
    }

    let events_json: Vec<Value> = events
        .iter()
        .map(|e| serde_json::to_value(e).expect("serialize event"))
        .collect();

    json!({
        "events": events_json,
        "diagnostics": diagnostics,
        "skipped": Value::Object(skipped),
    })
}

/// Detect which registered harnesses have readable data reachable from the
/// context. A harness is `supported` when its reader produced at least one
/// event; `unsupported` has no core representation and is always empty.
pub fn detect(opts: &Value) -> Value {
    let ctx = crate::parse::context_from_options(opts);
    let mut supported: Vec<String> = Vec::new();
    for reader in registered_readers() {
        let result = reader.read(&ctx);
        if !result.events.is_empty() {
            supported.push(reader.harness_id().to_owned());
        }
    }
    json!({
        "supported": supported,
        "unsupported": Value::Array(Vec::new()),
    })
}

//! Native coverage for date-only `since`/`until` bound resolution through the
//! production `parse_read_options` path (the same code the py/ruby direct-core
//! bindings run). These pin how a `YYYY-MM-DD` bound maps to an epoch instant in
//! a requested IANA zone: local-midnight boundaries and their inclusivity, DST
//! spring-forward and fall-back, a half-hour zone, an unknown-zone UTC fallback,
//! and the tz-omitted system-zone default. The TS suite mirrors these cases
//! against `src/read.ts`; both must agree at the calendar-day boundary.

use serde_json::json;
use skopli_wire::read::{parse_read_options, read_usage};

/// Resolve `{since?, until?, tz?}` to the `(since_ms, until_ms)` the read loop
/// filters on.
fn bounds(opts: serde_json::Value) -> (Option<i64>, Option<i64>) {
    let parsed = parse_read_options(&opts).expect("valid options");
    (parsed.since_ms, parsed.until_ms)
}

fn ms(iso: &str) -> i64 {
    skopli_core::readers::shared::date_parse_ms(iso).expect("parse iso")
}

#[test]
fn explicit_zone_since_is_local_midnight() {
    // 2026-08-01 in America/Los_Angeles (PDT, UTC-7) is 07:00Z that day.
    let (since, until) = bounds(json!({ "since": "2026-08-01", "tz": "America/Los_Angeles" }));
    assert_eq!(since, Some(ms("2026-08-01T07:00:00.000Z")));
    assert_eq!(until, None);
}

#[test]
fn explicit_zone_until_is_next_local_midnight_exclusive() {
    // date-only `until` is the exclusive end: midnight of the FOLLOWING local
    // day. 2026-08-02 00:00 LA (PDT) is 2026-08-02T07:00Z.
    let (_, until) = bounds(json!({ "until": "2026-08-01", "tz": "America/Los_Angeles" }));
    assert_eq!(until, Some(ms("2026-08-02T07:00:00.000Z")));
}

#[test]
fn since_inclusivity_at_and_around_local_midnight() {
    // An event exactly at `since` local midnight is kept (`at < since` drops);
    // one instant before is dropped; one instant after is kept.
    let (since, _) = bounds(json!({ "since": "2026-08-01", "tz": "America/Los_Angeles" }));
    let midnight = since.unwrap();
    let at = ms("2026-08-01T07:00:00.000Z");
    let before = ms("2026-08-01T06:59:59.000Z");
    let after = ms("2026-08-01T07:00:01.000Z");
    assert!(before < midnight, "just before local midnight is excluded");
    assert!(at >= midnight, "exactly at local midnight is included");
    assert!(after >= midnight, "just after local midnight is included");
}

#[test]
fn until_excludes_the_following_local_midnight() {
    // date-only `until` at the following local midnight excludes an event at
    // that instant (`at >= until` drops) but keeps the instant just before.
    let (_, until) = bounds(json!({ "until": "2026-08-01", "tz": "America/Los_Angeles" }));
    let end = until.unwrap();
    let at_end = ms("2026-08-02T07:00:00.000Z");
    let before_end = ms("2026-08-02T06:59:59.000Z");
    assert!(at_end >= end, "the following local midnight is excluded");
    assert!(before_end < end, "the instant before it is included");
}

#[test]
fn spring_forward_since_takes_first_valid_instant() {
    // 2026-04-05 in America/Santiago springs forward: 00:00 does not exist, so
    // local midnight resolves to the first valid instant (01:00 local = 04:00Z,
    // Santiago is UTC-4 in standard time before the jump / UTC-3 after).
    let (since, _) = bounds(json!({ "since": "2026-04-05", "tz": "America/Santiago" }));
    // The clocks skip 00:00->01:00; the first valid instant is 2026-04-05T04:00Z.
    assert_eq!(since, Some(ms("2026-04-05T04:00:00.000Z")));
}

#[test]
fn fall_back_since_takes_first_occurrence() {
    // 2026-11-01 in America/New_York, well after the fall-back; ordinary local
    // midnight EDT (UTC-4) is 04:00Z. (The fold is at 01:00 local, not at
    // midnight, so midnight is unambiguous here.)
    let (since, _) = bounds(json!({ "since": "2026-11-01", "tz": "America/New_York" }));
    assert_eq!(since, Some(ms("2026-11-01T04:00:00.000Z")));
}

#[test]
fn half_hour_zone_midnight() {
    // 2026-01-01 in Asia/Kolkata (UTC+5:30) local midnight is 2025-12-31T18:30Z.
    let (since, _) = bounds(json!({ "since": "2026-01-01", "tz": "Asia/Kolkata" }));
    assert_eq!(since, Some(ms("2025-12-31T18:30:00.000Z")));
}

#[test]
fn unknown_zone_falls_back_to_utc() {
    // An unknown zone degrades to UTC midnight rather than erroring.
    let (since, _) = bounds(json!({ "since": "2026-08-01", "tz": "Not/AZone" }));
    assert_eq!(since, Some(ms("2026-08-01T00:00:00.000Z")));
}

#[test]
fn explicit_utc_is_utc_midnight() {
    let (since, until) =
        bounds(json!({ "since": "2026-08-01", "until": "2026-08-01", "tz": "UTC" }));
    assert_eq!(since, Some(ms("2026-08-01T00:00:00.000Z")));
    assert_eq!(until, Some(ms("2026-08-02T00:00:00.000Z")));
}

#[test]
fn full_iso_bound_is_unaffected_by_tz() {
    // A full ISO instant is parsed as-is regardless of tz.
    let (since, _) = bounds(json!({
        "since": "2026-08-01T00:00:00Z",
        "tz": "America/Los_Angeles",
    }));
    assert_eq!(since, Some(ms("2026-08-01T00:00:00.000Z")));
}

#[test]
fn omitted_tz_matches_explicit_system_zone() {
    // The tz-omitted default resolves the system zone, so an omitted-tz bound
    // equals one that names the system zone explicitly. Portable: it asserts
    // equality between the two calls, not any particular host zone.
    let (omitted, _) = bounds(json!({ "since": "2026-08-01" }));
    if let Some(name) = jiff::tz::TimeZone::system().iana_name() {
        let (explicit, _) = bounds(json!({ "since": "2026-08-01", "tz": name }));
        assert_eq!(
            omitted, explicit,
            "omitted-tz must equal explicit system zone"
        );
    } else {
        let (utc, _) = bounds(json!({ "since": "2026-08-01", "tz": "UTC" }));
        assert_eq!(omitted, utc, "no system iana name resolves to UTC");
    }
}

// The bound-resolution assertions above pin how a `YYYY-MM-DD` maps to an
// instant. The cases below drive that instant through the production filter loop
// in `read_usage` over a real reader input, so a reversed inequality or a
// dropped bound in `read.rs` is caught: they assert exactly which event
// timestamps survive `since`/`until` in an explicit IANA zone.

/// A minimal claude-format JSONL fixture: one assistant usage record per
/// timestamp, each with a distinct `id` (the reader dedups on message id) and
/// finite tokens. This is the same on-disk shape the golden claude case uses;
/// the reader is pointed at it via `CLAUDE_CONFIG_DIR`.
fn write_claude_fixture(dir: &std::path::Path, timestamps: &[&str]) {
    let projects = dir.join("projects").join("proj");
    std::fs::create_dir_all(&projects).expect("create fixture dir");
    let mut lines = String::new();
    for (i, ts) in timestamps.iter().enumerate() {
        let line = json!({
            "type": "assistant",
            "sessionId": "sess",
            "uuid": format!("u{i}"),
            "timestamp": ts,
            "message": {
                "id": format!("msg{i}"),
                "model": "claude-sonnet-4-5-20250929",
                "usage": { "input_tokens": 1, "output_tokens": 1 }
            }
        });
        lines.push_str(&serde_json::to_string(&line).unwrap());
        lines.push('\n');
    }
    std::fs::write(projects.join("session.jsonl"), lines).expect("write fixture");
}

/// A unique temp dir for one test's fixture, keyed by process id + a label so
/// parallel test runs never collide. Cleaned before use.
fn fixture_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("skopli-wire-read-{}-{label}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Read the claude fixture at `dir` with the given `{since?, until?, tz?}`,
/// returning the surviving event timestamps in read order.
fn filtered_timestamps(dir: &std::path::Path, mut opts: serde_json::Value) -> Vec<String> {
    let obj = opts.as_object_mut().unwrap();
    obj.insert("home".to_owned(), json!("/nonexistent"));
    obj.insert(
        "env".to_owned(),
        json!({ "CLAUDE_CONFIG_DIR": dir.to_string_lossy() }),
    );
    obj.insert("harnesses".to_owned(), json!(["claude"]));
    let parsed = parse_read_options(&opts).expect("valid options");
    let envelope = read_usage(&parsed);
    envelope["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|e| e["timestamp"].as_str().unwrap_or("").to_owned())
        .collect()
}

#[test]
fn filter_loop_applies_since_bound_in_explicit_zone() {
    // `since` 2026-08-01 in America/Los_Angeles (PDT) is 07:00Z. The event just
    // before the bound is dropped; the one exactly at it and the one just after
    // are kept.
    let dir = fixture_dir("since");
    write_claude_fixture(
        &dir,
        &[
            "2026-08-01T06:59:59.000Z",
            "2026-08-01T07:00:00.000Z",
            "2026-08-01T07:00:01.000Z",
        ],
    );
    let got = filtered_timestamps(
        &dir,
        json!({ "since": "2026-08-01", "tz": "America/Los_Angeles" }),
    );
    assert_eq!(
        got,
        vec![
            "2026-08-01T07:00:00.000Z".to_owned(),
            "2026-08-01T07:00:01.000Z".to_owned(),
        ],
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn filter_loop_applies_until_bound_in_explicit_zone() {
    // date-only `until` 2026-08-01 in America/Los_Angeles is the exclusive end at
    // the following local midnight, 2026-08-02T07:00Z. The event just before it
    // is kept; the one exactly at it and the one just after are dropped.
    let dir = fixture_dir("until");
    write_claude_fixture(
        &dir,
        &[
            "2026-08-02T06:59:59.000Z",
            "2026-08-02T07:00:00.000Z",
            "2026-08-02T07:00:01.000Z",
        ],
    );
    let got = filtered_timestamps(
        &dir,
        json!({ "until": "2026-08-01", "tz": "America/Los_Angeles" }),
    );
    assert_eq!(got, vec!["2026-08-02T06:59:59.000Z".to_owned()]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn filter_loop_keeps_both_folds_of_a_fall_back_day() {
    // 2026-11-01 in America/New_York is the fall-back day: 01:30 local happens
    // twice. 05:30Z is the first 01:30 (still EDT, UTC-4) and 06:30Z is the
    // repeated 01:30 (now EST, UTC-5). Both are local November 1, so filtering by
    // since/until 2026-11-01 must keep BOTH across the fold.
    let dir = fixture_dir("fallback");
    write_claude_fixture(
        &dir,
        &["2026-11-01T05:30:00.000Z", "2026-11-01T06:30:00.000Z"],
    );
    let got = filtered_timestamps(
        &dir,
        json!({ "since": "2026-11-01", "until": "2026-11-01", "tz": "America/New_York" }),
    );
    assert_eq!(
        got,
        vec![
            "2026-11-01T05:30:00.000Z".to_owned(),
            "2026-11-01T06:30:00.000Z".to_owned(),
        ],
    );
    std::fs::remove_dir_all(&dir).ok();
}

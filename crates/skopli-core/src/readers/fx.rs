use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, is_record, parse_timestamp_to_iso, read_jsonl_lines,
};
use crate::types::{TokenCounts, UsageEvent};

/// fx persists usage in two stores under `~/.fx/sessions`: a cumulative
/// `usage-v2.json` sidecar per session directory (one event per `models[]`
/// entry, `message_id` `{session_id}:{model}` so a rewritten snapshot replaces
/// its earlier events) and, from fx 0.0.16, an append-only
/// `v2/<session_id>/log.jsonl` whose `usage` records restate the session's
/// cumulative counters and are booked as the delta from the previous record.
pub struct FxReader;

impl Reader for FxReader {
    fn harness_id(&self) -> &'static str {
        HARNESS
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        read_fx(&fx_sessions_dir(ctx.home()))
    }
}

const HARNESS: &str = "fx";
const SIDECAR_FILE: &str = "usage-v2.json";
const V2_LOG_FILE: &str = "log.jsonl";

pub fn fx_sessions_dir(home: &Path) -> PathBuf {
    home.join(".fx").join("sessions")
}

const BILLING_STATES: [&str; 4] = ["complete", "pending", "incomplete", "legacy"];

fn required_u64(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

/// `Ok(None)` for an absent or null counter, `Err(())` for a populated one
/// that is not a u64.
fn optional_u64(value: Option<&Value>) -> Result<Option<u64>, ()> {
    match value {
        None => Ok(None),
        Some(v) if v.is_null() => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or(()),
    }
}

fn optional_cost(value: Option<&Value>) -> Option<f64> {
    value?.as_f64().filter(|n| n.is_finite())
}

/// `{ms}-{ns}-{16 lowercase hex}`.
fn is_valid_session_id(id: &str) -> bool {
    let mut parts = id.split('-');
    let (Some(ms), Some(ns), Some(hex), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let is_decimal = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let is_hex16 = hex.len() == 16
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    is_decimal(ms) && is_decimal(ns) && is_hex16
}

fn timestamp_from_session_id(session_id: &str) -> Option<String> {
    let prefix = session_id.split('-').next()?;
    let ms: f64 = prefix.parse().ok()?;
    parse_timestamp_to_iso(&Value::from(ms))
}

/// One event for a `models[]` entry, `Err(())` when a consumed field is
/// invalid.
fn model_event(entry: &Value, session_id: &str, timestamp: &str) -> Result<UsageEvent, ()> {
    if !is_record(entry) {
        return Err(());
    }
    let model = entry
        .get("model")
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .ok_or(())?;
    let input = required_u64(entry.get("input_tokens")).ok_or(())?;
    let output = required_u64(entry.get("output_tokens")).ok_or(())?;
    let cache_read = required_u64(entry.get("cache_read_tokens")).ok_or(())?;
    let cache_write = required_u64(entry.get("cache_write_tokens")).ok_or(())?;
    let reasoning = optional_u64(entry.get("reasoning_tokens"))?.unwrap_or(0);
    let calls = optional_u64(entry.get("request_count"))?;
    let message_id = format!("{session_id}:{model}");
    Ok(UsageEvent {
        harness: HARNESS.to_owned(),
        timestamp: timestamp.to_owned(),
        session_id: session_id.to_owned(),
        message_id,
        turn: false,
        subagent: false,
        model,
        tokens: TokenCounts {
            input,
            output,
            cache_read,
            cache_write,
            cache_write1h: None,
            reasoning,
        },
        calls,
        cost_usd: optional_cost(entry.get("total_cost")),
        workspace: None,
        title: None,
    })
}

struct SidecarRead {
    events: Vec<UsageEvent>,
    has_incidents: bool,
}

/// Validate only the fields the reader consumes: an envelope of exactly
/// `schema_version` / `session_id` / `snapshot`, `schema_version` 1 at both
/// levels, a session id equal to `dir_name`, a known `billing` state and
/// unique model names. Other producer fields are not gated, so benign
/// additions do not reject the sidecar.
fn validate_sidecar(file: &str, dir_name: &str) -> Result<SidecarRead, ()> {
    let content = std::fs::read_to_string(file).map_err(|_| ())?;
    let envelope: Value = serde_json::from_str(&content).map_err(|_| ())?;
    let envelope = envelope.as_object().ok_or(())?;
    if envelope.len() != 3
        || !envelope.contains_key("schema_version")
        || !envelope.contains_key("session_id")
        || !envelope.contains_key("snapshot")
    {
        return Err(());
    }
    if envelope.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(());
    }
    let session_id = envelope
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or(())?;
    if session_id != dir_name || !is_valid_session_id(session_id) {
        return Err(());
    }
    let snapshot = envelope
        .get("snapshot")
        .and_then(Value::as_object)
        .ok_or(())?;
    if snapshot.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(());
    }
    let billing = snapshot.get("billing").and_then(Value::as_str).ok_or(())?;
    if !BILLING_STATES.contains(&billing) {
        return Err(());
    }
    let models = snapshot.get("models").and_then(Value::as_array).ok_or(())?;
    let timestamp = timestamp_from_session_id(session_id).ok_or(())?;
    let mut events = Vec::with_capacity(models.len());
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in models.iter() {
        let event = model_event(entry, session_id, &timestamp)?;
        if !seen.insert(event.model.clone()) {
            return Err(());
        }
        events.push(event);
    }
    Ok(SidecarRead {
        events,
        has_incidents: has_incidents(snapshot),
    })
}

fn has_incidents(snapshot: &serde_json::Map<String, Value>) -> bool {
    snapshot
        .get("incidents")
        .and_then(Value::as_array)
        .is_some_and(|incidents| !incidents.is_empty())
}

/// A schema violation is one diagnostic against `label`; a non-empty
/// `incidents[]` is one informational warning and skips nothing.
fn read_sidecar(
    file: &str,
    dir_name: &str,
    label: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    match validate_sidecar(file, dir_name) {
        Ok(read) => {
            if read.has_incidents {
                warnings.push(ReaderWarning {
                    message: format!(
                        "{HARNESS} session {dir_name} reports completeness incidents\n"
                    ),
                });
            }
            read.events
        }
        Err(()) => {
            skip(label, skipped, warnings);
            Vec::new()
        }
    }
}

/// Push one malformed-sidecar diagnostic for `label`.
fn skip(label: &str, skipped: &mut Vec<String>, warnings: &mut Vec<ReaderWarning>) {
    warnings.push(ReaderWarning {
        message: format!("skipping malformed {HARNESS} record {label}\n"),
    });
    skipped.push(label.to_owned());
}

/// One event per `usage` record, holding the difference from the previous
/// record in file order (a record whose counters fell is a fresh baseline, an
/// unchanged restatement yields nothing); the last usage event before a
/// `turn_committed` record is the turn's event. The model comes from the
/// latest `prefs` record, the workspace and title from the session's final
/// ones.
fn read_v2_log(
    file: &Path,
    session_id: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let file = file.to_string_lossy();
    let Some(lines) = read_jsonl_lines(&file, skipped, warnings) else {
        return Vec::new();
    };
    let mut model = String::new();
    let mut title = None;
    let mut workspace = None;
    let mut previous = TokenCounts::default();
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut open_turn: Option<usize> = None;
    for line in lines {
        let label = || format!("{file}:{}", line.index + 1);
        let record = &line.value;
        if !is_record(record) {
            skip(&label(), skipped, warnings);
            continue;
        }
        let value = record.get("value");
        match (
            record.get("kind").and_then(Value::as_str),
            record.get("key").and_then(Value::as_str),
        ) {
            (Some("session_created"), _) => workspace = record.get("workspace").and_then(as_string),
            (Some("turn_committed"), _) => {
                if let Some(index) = open_turn.take() {
                    events[index].turn = true;
                }
            }
            (Some("set"), Some("prefs")) => {
                if let Some(m) = value.and_then(|v| v.get("model")).and_then(as_string) {
                    model = m;
                }
            }
            (Some("set"), Some("title")) => title = value.and_then(as_string),
            (Some("set"), Some("usage")) => match v2_usage(record) {
                Some((snapshot, timestamp)) => {
                    let tokens = delta(&snapshot, &previous);
                    previous = snapshot;
                    if tokens == TokenCounts::default() {
                        continue;
                    }
                    events.push(UsageEvent {
                        harness: HARNESS.to_owned(),
                        timestamp,
                        session_id: session_id.to_owned(),
                        message_id: format!("{session_id}:{}", line.index + 1),
                        turn: false,
                        subagent: false,
                        model: model.clone(),
                        tokens,
                        calls: Some(1),
                        cost_usd: None,
                        workspace: None,
                        title: None,
                    });
                    open_turn = Some(events.len() - 1);
                }
                None => skip(&label(), skipped, warnings),
            },
            _ => {}
        }
    }
    for event in &mut events {
        event.workspace = workspace.clone();
        event.title = title.clone();
    }
    events
}

/// `snapshot` minus `previous` per counter, or `snapshot` itself when any
/// counter went down.
fn delta(snapshot: &TokenCounts, previous: &TokenCounts) -> TokenCounts {
    let pairs = [
        (snapshot.input, previous.input),
        (snapshot.output, previous.output),
        (snapshot.cache_read, previous.cache_read),
        (snapshot.cache_write, previous.cache_write),
        (snapshot.reasoning, previous.reasoning),
    ];
    if pairs.iter().any(|(now, before)| now < before) {
        return snapshot.clone();
    }
    TokenCounts {
        input: snapshot.input - previous.input,
        output: snapshot.output - previous.output,
        cache_read: snapshot.cache_read - previous.cache_read,
        cache_write: snapshot.cache_write - previous.cache_write,
        cache_write1h: None,
        reasoning: snapshot.reasoning - previous.reasoning,
    }
}

/// The token counts and ISO timestamp of one v2 `usage` record, `None` when a
/// required counter is not a u64, an optional one is populated with a non-u64,
/// or `ts` is not a number.
fn v2_usage(record: &Value) -> Option<(TokenCounts, String)> {
    let value = record.get("value").filter(|v| is_record(v))?;
    let optional = |key: &str| optional_u64(value.get(key)).ok().map(|v| v.unwrap_or(0));
    let tokens = TokenCounts {
        input: required_u64(value.get("input_tokens"))?,
        output: required_u64(value.get("output_tokens"))?,
        cache_read: optional("cache_read_tokens")?,
        cache_write: optional("cache_write_tokens")?,
        cache_write1h: None,
        reasoning: optional("reasoning_tokens")?,
    };
    let timestamp = record
        .get("ts")
        .filter(|ts| ts.is_number())
        .and_then(parse_timestamp_to_iso)?;
    Some((tokens, timestamp))
}

/// The immediate subdirectory names of `dir`, sorted; depth one, so a nested
/// file cannot shift ordinals.
fn immediate_session_dirs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// Sidecar diagnostics are labelled `{path}:usage-v2:{ordinal}` over the
/// sorted session directories; a missing directory or sidecar is silent.
pub fn read_fx(sessions_dir: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let mut ordinal = 0usize;
    for name in immediate_session_dirs(sessions_dir) {
        let sidecar = sessions_dir.join(&name).join(SIDECAR_FILE);
        if !sidecar.is_file() {
            continue;
        }
        ordinal += 1;
        let file = sidecar.to_string_lossy().into_owned();
        let label = format!("{file}:usage-v2:{ordinal}");
        events.extend(read_sidecar(
            &file,
            &name,
            &label,
            &mut skipped,
            &mut warnings,
        ));
    }

    let v2 = sessions_dir.join("v2");
    for name in immediate_session_dirs(&v2) {
        let log = v2.join(&name).join(V2_LOG_FILE);
        if log.is_file() {
            events.extend(read_v2_log(&log, &name, &mut skipped, &mut warnings));
        }
    }

    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readers::shared::{scratch_dir, test_home};
    use std::fs;

    #[test]
    fn sessions_dir_is_home_dot_fx_sessions() {
        assert_eq!(
            fx_sessions_dir(&test_home()),
            test_home().join(".fx").join("sessions")
        );
    }

    #[test]
    fn timestamp_derives_from_session_id_ms_prefix() {
        let id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        assert_eq!(
            timestamp_from_session_id(id).as_deref(),
            Some("2026-02-02T02:40:00.000Z")
        );
    }

    #[test]
    fn valid_session_id_requires_all_three_components() {
        assert!(is_valid_session_id(
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718"
        ));
    }

    #[test]
    fn invalid_session_ids_are_rejected() {
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("nosep"));
        assert!(!is_valid_session_id("abc-1-a1b2c3d4e5f60718"));
        assert!(!is_valid_session_id("1770000000000-x-a1b2c3d4e5f60718"));
        assert!(!is_valid_session_id("1770000000000--a1b2c3d4e5f60718"));
        assert!(!is_valid_session_id("1770000000000-1-not-hex"));
        assert!(!is_valid_session_id(
            "1770000000000-1-a1b2c3d4e5f60718-extra"
        ));
        assert!(!is_valid_session_id("1770000000000-1-a1b2c3d4e5f6071"));
        assert!(!is_valid_session_id("1770000000000-1-A1B2C3D4E5F60718"));
        assert!(!is_valid_session_id("1770000000000-1-a1b2c3d4e5f6071g"));
    }

    fn write_sidecar(sessions: &Path, session_id: &str, snapshot: Value) {
        let dir = sessions.join(session_id);
        fs::create_dir_all(&dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": session_id,
            "snapshot": snapshot,
        });
        fs::write(dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
    }

    #[test]
    fn empty_sessions_dir_yields_no_events() {
        let dir = scratch_dir("empty");
        let sessions = dir.join("sessions");
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        assert!(result.skipped.is_empty());
        assert!(result.warnings.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn one_event_per_model_entry_with_real_tokens() {
        let dir = scratch_dir("multi-model");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.99,
                "input_tokens": 999,
                "output_tokens": 999,
                "cache_read_tokens": 999,
                "cache_write_tokens": 999,
                "reasoning_tokens": 999,
                "request_count": 99,
                "models": [
                    {
                        "model": "openai/gpt-5.4",
                        "total_cost": 0.03,
                        "input_tokens": 20,
                        "output_tokens": 8,
                        "cache_read_tokens": 4,
                        "cache_write_tokens": 2,
                        "reasoning_tokens": 4,
                        "request_count": 1
                    },
                    {
                        "model": "anthropic/claude-sonnet-4-5",
                        "total_cost": 0.02,
                        "input_tokens": 10,
                        "output_tokens": 4,
                        "cache_read_tokens": 2,
                        "cache_write_tokens": 1,
                        "reasoning_tokens": 0,
                        "request_count": 1
                    }
                ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(events.len(), 2);
        let first = &events[0];
        assert_eq!(first.model, "openai/gpt-5.4");
        assert_eq!(first.tokens.input, 20);
        assert_eq!(first.tokens.output, 8);
        assert_eq!(first.tokens.cache_read, 4);
        assert_eq!(first.tokens.cache_write, 2);
        assert_eq!(first.tokens.reasoning, 4);
        assert_eq!(first.calls, Some(1));
        assert_eq!(first.cost_usd, Some(0.03));
        assert_eq!(first.timestamp, "2026-02-02T02:40:00.000Z");
        assert!(!first.turn);
        assert!(!first.subagent);
        assert_eq!(
            first.session_id,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718"
        );
        assert_eq!(
            first.message_id,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718:openai/gpt-5.4"
        );
        let second = &events[1];
        assert_eq!(
            second.message_id,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718:anthropic/claude-sonnet-4-5"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_snapshot_reads_with_absent_reasoning_and_calls() {
        let dir = scratch_dir("legacy");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000001000-1770000000000000001-b1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "legacy",
                "total_cost": 0.01,
                "input_tokens": 10,
                "output_tokens": 4,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0,
                "reasoning_tokens": null,
                "request_count": null,
                "models": [
                    {
                        "model": "openai/gpt-5.4",
                        "total_cost": 0.01,
                        "input_tokens": 10,
                        "output_tokens": 4,
                        "cache_read_tokens": 0,
                        "cache_write_tokens": 0,
                        "reasoning_tokens": null,
                        "request_count": null
                    }
                ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tokens.reasoning, 0);
        assert_eq!(events[0].calls, None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pending_billing_is_still_read() {
        let dir = scratch_dir("pending");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000002000-1770000000000000002-c1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "pending",
                "total_cost": 0.02,
                "input_tokens": 15,
                "output_tokens": 5,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0,
                "reasoning_tokens": 0,
                "request_count": 1,
                "models": [
                    {
                        "model": "anthropic/claude-sonnet-4-5",
                        "total_cost": 0.02,
                        "input_tokens": 15,
                        "output_tokens": 5,
                        "cache_read_tokens": 0,
                        "cache_write_tokens": 0,
                        "reasoning_tokens": 0,
                        "request_count": 1
                    }
                ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(events.len(), 1, "pending usage that exists is still usage");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_schema_version_is_a_diagnostic() {
        let dir = scratch_dir("bad-version");
        let sessions = dir.join("sessions");
        let session_id = "1770000003000-1770000000000000003-d1b2c3d4e5f60718";
        let session_dir = sessions.join(session_id);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 2,
            "session_id": session_id,
            "snapshot": { "schema_version": 2, "models": [] },
        });
        let file = session_dir.join(SIDECAR_FILE);
        fs::write(&file, envelope.to_string()).unwrap();
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        let file_str = file.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![format!("{file_str}:usage-v2:1")]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed fx record {file_str}:usage-v2:1\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_json_is_a_diagnostic() {
        let dir = scratch_dir("bad-json");
        let sessions = dir.join("sessions");
        let session_dir = sessions.join("1770000004000-1770000000000000004-e1b2c3d4e5f60718");
        fs::create_dir_all(&session_dir).unwrap();
        let file = session_dir.join(SIDECAR_FILE);
        fs::write(&file, b"{not valid json").unwrap();
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        let file_str = file.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![format!("{file_str}:usage-v2:1")]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_models_array_is_silent() {
        let dir = scratch_dir("empty-models");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000005000-1770000000000000005-f1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.0,
                "input_tokens": 0,
                "output_tokens": 0,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0,
                "reasoning_tokens": 0,
                "request_count": 0,
                "models": []
            }),
        );
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        assert!(result.warnings.is_empty());
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn incomplete_billing_is_still_read() {
        let dir = scratch_dir("incomplete");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000006000-1770000000000000006-a2b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "incomplete",
                "total_cost": 0.02,
                "models": [ model_aggregate("openai/gpt-5.4") ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(
            events.len(),
            1,
            "incomplete usage that exists is still usage"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_empty_incidents_draw_an_informational_warning_without_skipping() {
        let dir = scratch_dir("incidents");
        let sessions = dir.join("sessions");
        let session_id = "1770000007000-1770000000000000007-b2b2c3d4e5f60718";
        write_sidecar(
            &sessions,
            session_id,
            serde_json::json!({
                "schema_version": 1,
                "billing": "incomplete",
                "total_cost": 0.02,
                "incidents": [ { "occurred_at_ms": 1770000007000i64, "completeness": "partial" } ],
                "models": [ model_aggregate("openai/gpt-5.4") ]
            }),
        );
        let result = read_fx(&sessions);
        assert_eq!(result.events.len(), 1, "incidents do not skip usage");
        assert!(result.skipped.is_empty(), "an incident is not a skip");
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result.warnings[0].message,
            format!("fx session {session_id} reports completeness incidents\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_or_absent_incidents_are_silent() {
        let dir = scratch_dir("incidents-empty");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000008000-1770000000000000008-c2b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.02,
                "incidents": [],
                "models": [ model_aggregate("openai/gpt-5.4") ]
            }),
        );
        let result = read_fx(&sessions);
        assert_eq!(result.events.len(), 1);
        assert!(result.warnings.is_empty(), "empty incidents is silent");
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_model_names_are_a_diagnostic() {
        let dir = scratch_dir("duplicate-model");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000009000-1770000000000000009-d2b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.04,
                "models": [
                    model_aggregate("openai/gpt-5.4"),
                    model_aggregate("openai/gpt-5.4")
                ]
            }),
        );
        assert_only_diagnostic(&sessions, "duplicate_model_names");
        fs::remove_dir_all(&dir).ok();
    }

    /// A well-formed model aggregate whose required u64 counters are all present.
    fn model_aggregate(model: &str) -> Value {
        serde_json::json!({
            "model": model,
            "total_cost": 0.02,
            "input_tokens": 10,
            "output_tokens": 4,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "reasoning_tokens": 0,
            "request_count": 1
        })
    }

    /// A snapshot with one model aggregate mutated by `mutate`, used to prove a
    /// specific malformed field skips the whole sidecar.
    fn snapshot_with_model(mutate: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Value {
        let mut aggregate = model_aggregate("openai/gpt-5.4");
        mutate(aggregate.as_object_mut().unwrap());
        serde_json::json!({
            "schema_version": 1,
            "billing": "complete",
            "total_cost": 0.02,
            "models": [ aggregate ]
        })
    }

    fn assert_only_diagnostic(sessions: &Path, case: &str) {
        let result = read_fx(sessions);
        assert!(result.events.is_empty(), "{case}: expected no events");
        assert_eq!(
            result.skipped.len(),
            1,
            "{case}: expected one skipped sidecar"
        );
        assert_eq!(result.warnings.len(), 1, "{case}: expected one warning");
    }

    #[test]
    fn malformed_model_aggregates_are_a_diagnostic() {
        // `None` removes the cell; `Some` overwrites it.
        let cases = [
            ("missing required token", "input_tokens", None),
            ("null required token", "output_tokens", Some(Value::Null)),
            ("string token", "cache_read_tokens", Some(Value::from("12"))),
            (
                "negative token",
                "cache_write_tokens",
                Some(Value::from(-1)),
            ),
            ("fractional token", "input_tokens", Some(Value::from(1.5))),
            ("blank model name", "model", Some(Value::from("   "))),
            ("missing model cell", "model", None),
            (
                "malformed request count",
                "request_count",
                Some(Value::from("many")),
            ),
        ];
        for (name, field, replacement) in cases {
            let dir = scratch_dir("bad-aggregate");
            let sessions = dir.join("sessions");
            write_sidecar(
                &sessions,
                "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
                snapshot_with_model(|m| match replacement {
                    Some(v) => {
                        m.insert(field.into(), v);
                    }
                    None => {
                        m.remove(field);
                    }
                }),
            );
            assert_only_diagnostic(&sessions, name);
            fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn overflowing_token_field_is_a_diagnostic() {
        let dir = scratch_dir("overflow-token");
        let sessions = dir.join("sessions");
        let session_id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        let session_dir = sessions.join(session_id);
        fs::create_dir_all(&session_dir).unwrap();
        let raw = format!(
            r#"{{"schema_version":1,"session_id":"{session_id}","snapshot":{{"schema_version":1,"billing":"complete","total_cost":0.02,"models":[{{"model":"openai/gpt-5.4","total_cost":0.02,"input_tokens":18446744073709551616,"output_tokens":4,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"request_count":1}}]}}}}"#
        );
        fs::write(session_dir.join(SIDECAR_FILE), raw).unwrap();
        assert_only_diagnostic(&sessions, "overflowing_token_field");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extra_top_level_key_is_a_diagnostic() {
        let dir = scratch_dir("extra-key");
        let sessions = dir.join("sessions");
        let session_id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        let session_dir = sessions.join(session_id);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": session_id,
            "snapshot": { "schema_version": 1, "billing": "complete", "models": [] },
            "extra": true,
        });
        fs::write(session_dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
        assert_only_diagnostic(&sessions, "extra_top_level_key");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn session_id_directory_mismatch_is_a_diagnostic() {
        let dir = scratch_dir("dir-mismatch");
        let sessions = dir.join("sessions");
        let dir_name = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        let session_dir = sessions.join(dir_name);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": "1770000000000-1770000000000000000-b1b2c3d4e5f60718",
            "snapshot": { "schema_version": 1, "billing": "complete", "models": [] },
        });
        fs::write(session_dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
        assert_only_diagnostic(&sessions, "session_id_directory_mismatch");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_snapshots_are_a_diagnostic() {
        let cases = [
            (
                "missing snapshot schema version",
                serde_json::json!({ "billing": "complete", "models": [] }),
            ),
            (
                "absent models",
                serde_json::json!({ "schema_version": 1, "billing": "complete" }),
            ),
            (
                "non-array models",
                serde_json::json!({
                    "schema_version": 1,
                    "billing": "complete",
                    "models": { "model": "openai/gpt-5.4" }
                }),
            ),
            (
                "unknown billing",
                serde_json::json!({ "schema_version": 1, "billing": "mystery", "models": [] }),
            ),
        ];
        for (name, snapshot) in cases {
            let dir = scratch_dir("bad-snapshot");
            let sessions = dir.join("sessions");
            write_sidecar(
                &sessions,
                "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
                snapshot,
            );
            assert_only_diagnostic(&sessions, name);
            fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn invalid_session_dir_name_is_a_diagnostic() {
        let dir = scratch_dir("bad-dir-name");
        let sessions = dir.join("sessions");
        let dir_name = "not-a-valid-id";
        let session_dir = sessions.join(dir_name);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": dir_name,
            "snapshot": { "schema_version": 1, "billing": "complete", "models": [] },
        });
        fs::write(session_dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
        assert_only_diagnostic(&sessions, "invalid_session_dir_name");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nested_sidecar_is_ignored_and_does_not_shift_ordinals() {
        let dir = scratch_dir("nested");
        let sessions = dir.join("sessions");
        let session_id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        write_sidecar(&sessions, session_id, snapshot_with_model(|_| {}));
        let nested = sessions.join(session_id).join("deeper");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(SIDECAR_FILE), b"{not valid json").unwrap();
        let result = read_fx(&sessions);
        assert_eq!(result.events.len(), 1);
        assert!(result.skipped.is_empty(), "nested file must be ignored");
        assert!(result.warnings.is_empty());
        let expected = sessions
            .join(session_id)
            .join(SIDECAR_FILE)
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            result.events[0].message_id,
            format!("{session_id}:openai/gpt-5.4")
        );
        assert!(expected.ends_with("usage-v2.json"));
        fs::remove_dir_all(&dir).ok();
    }

    fn write_v2_log(sessions: &Path, session_id: &str, lines: &[&str]) {
        let dir = sessions.join("v2").join(session_id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(V2_LOG_FILE), lines.join("\n") + "\n").unwrap();
    }

    const V2_CREATED: &str = r#"{"v":1,"seq":1,"ts":1793564111921,"kind":"session_created","id":"vi5FAUzM30R7","workspace":"/work/p-limit","role":"root","host":"ask","crc":"d019f95a"}"#;
    const V2_PREFS: &str = r#"{"v":1,"seq":2,"ts":1793564075976,"kind":"set","key":"prefs","value":{"provider":{"name":"local","binding":"845ca1b6"},"model":"qwen","effort":"auto","fast_mode":false},"crc":"97be1ae5"}"#;
    const V2_USAGE: &str = r#"{"v":1,"seq":5,"ts":1793564111920,"kind":"set","key":"usage","value":{"input_tokens":9016,"output_tokens":17,"cache_read_tokens":null,"cache_write_tokens":null,"reasoning_tokens":null},"crc":"d7666434"}"#;
    const V2_COMMIT: &str =
        r#"{"v":1,"seq":11,"ts":1793564111925,"kind":"turn_committed","turn":1,"crc":"b863673d"}"#;
    const V2_TITLE: &str = r#"{"v":1,"seq":12,"ts":1793564111923,"kind":"set","key":"title","value":"What does index.js in this project export? Answer","crc":"1ad98e57"}"#;

    #[test]
    fn v2_log_yields_one_event_per_usage_record() {
        let dir = scratch_dir("v2-log");
        let sessions = dir.join("sessions");
        write_v2_log(
            &sessions,
            "vi5FAUzM30R7",
            &[V2_CREATED, V2_PREFS, V2_USAGE, V2_COMMIT, V2_TITLE],
        );
        let result = read_fx(&sessions);
        assert!(result.skipped.is_empty(), "{:?}", result.skipped);
        assert_eq!(result.events.len(), 1);
        let event = &result.events[0];
        assert_eq!(event.session_id, "vi5FAUzM30R7");
        assert_eq!(event.message_id, "vi5FAUzM30R7:3");
        assert_eq!(event.model, "qwen");
        assert_eq!(event.timestamp, "2026-11-01T20:15:11.920Z");
        assert_eq!(event.tokens.input, 9016);
        assert_eq!(event.tokens.output, 17);
        assert_eq!(event.tokens.cache_read, 0);
        assert_eq!(event.tokens.cache_write, 0);
        assert_eq!(event.tokens.reasoning, 0);
        assert_eq!(event.calls, Some(1));
        assert!(event.turn);
        assert_eq!(event.workspace.as_deref(), Some("/work/p-limit"));
        assert_eq!(
            event.title.as_deref(),
            Some("What does index.js in this project export? Answer")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v2_log_without_a_usage_snapshot_is_silent() {
        let dir = scratch_dir("v2-no-usage");
        let sessions = dir.join("sessions");
        write_v2_log(&sessions, "vi5FAUzM30R7", &[V2_CREATED, V2_PREFS]);
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v2_log_books_each_restatement_as_the_delta_from_the_previous_one() {
        let dir = scratch_dir("v2-deltas");
        let sessions = dir.join("sessions");
        let later = r#"{"v":1,"seq":4,"ts":1793564125166,"kind":"set","key":"usage","value":{"input_tokens":36314,"output_tokens":99,"cache_read_tokens":null,"cache_write_tokens":null,"reasoning_tokens":null},"crc":"0a0a0a0a"}"#;
        let reset = r#"{"v":1,"seq":6,"ts":1793564130000,"kind":"set","key":"usage","value":{"input_tokens":5,"output_tokens":1,"cache_read_tokens":null,"cache_write_tokens":null,"reasoning_tokens":null},"crc":"0b0b0b0b"}"#;
        write_v2_log(
            &sessions,
            "vi5FAUzM30R7",
            &[
                V2_CREATED, V2_PREFS, V2_USAGE, later, V2_COMMIT, later, reset,
            ],
        );
        let result = read_fx(&sessions);
        assert!(result.skipped.is_empty(), "{:?}", result.skipped);
        let got: Vec<(u64, u64, &str, bool)> = result
            .events
            .iter()
            .map(|e| {
                (
                    e.tokens.input,
                    e.tokens.output,
                    e.timestamp.as_str(),
                    e.turn,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (9016, 17, "2026-11-01T20:15:11.920Z", false),
                (27298, 82, "2026-11-01T20:15:25.166Z", true),
                (5, 1, "2026-11-01T20:15:30.000Z", false),
            ]
        );
        assert!(result.events.iter().all(|e| e.calls == Some(1)));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v2_log_malformed_later_usage_keeps_the_earlier_events_and_warns() {
        let dir = scratch_dir("v2-later-malformed");
        let sessions = dir.join("sessions");
        let bad_usage = r#"{"v":1,"seq":9,"ts":1793564125166,"kind":"set","key":"usage","value":{"input_tokens":"x","output_tokens":2},"crc":"00000000"}"#;
        write_v2_log(
            &sessions,
            "vi5FAUzM30R7",
            &[V2_CREATED, V2_PREFS, V2_USAGE, bad_usage],
        );
        let result = read_fx(&sessions);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].tokens.input, 9016);
        assert_eq!(result.skipped.len(), 1, "{:?}", result.skipped);
        assert!(
            result.skipped[0].ends_with("log.jsonl:4"),
            "{}",
            result.skipped[0]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v2_log_that_is_not_utf8_is_a_skipped_file() {
        let dir = scratch_dir("v2-not-utf8");
        let sessions = dir.join("sessions");
        write_v2_log(&sessions, "vi5FAUzM30R7", &[V2_CREATED, V2_PREFS, V2_USAGE]);
        let log = sessions.join("v2").join("vi5FAUzM30R7").join(V2_LOG_FILE);
        let mut bytes = fs::read(&log).unwrap();
        bytes.extend_from_slice(b"\xff\xfe{\n");
        fs::write(&log, bytes).unwrap();
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        assert_eq!(result.skipped.len(), 1, "{:?}", result.skipped);
        assert!(
            result.skipped[0].ends_with("log.jsonl"),
            "{}",
            result.skipped[0]
        );
        assert_eq!(result.warnings.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sidecar_and_v2_sessions_in_one_profile_both_yield_events() {
        let dir = scratch_dir("mixed-layouts");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.03,
                "input_tokens": 20,
                "output_tokens": 8,
                "cache_read_tokens": 4,
                "cache_write_tokens": 2,
                "reasoning_tokens": 4,
                "request_count": 1,
                "models": [{
                    "model": "openai/gpt-5.4",
                    "total_cost": 0.03,
                    "input_tokens": 20,
                    "output_tokens": 8,
                    "cache_read_tokens": 4,
                    "cache_write_tokens": 2,
                    "reasoning_tokens": 4,
                    "request_count": 1
                }]
            }),
        );
        write_v2_log(&sessions, "vi5FAUzM30R7", &[V2_CREATED, V2_PREFS, V2_USAGE]);
        let result = read_fx(&sessions);
        assert!(result.skipped.is_empty(), "{:?}", result.skipped);
        let mut ids: Vec<&str> = result
            .events
            .iter()
            .map(|e| e.session_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
                "vi5FAUzM30R7"
            ]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v2_log_malformed_usage_or_line_is_a_diagnostic() {
        let dir = scratch_dir("v2-malformed");
        let sessions = dir.join("sessions");
        let bad_usage = r#"{"v":1,"seq":5,"ts":1793564111920,"kind":"set","key":"usage","value":{"input_tokens":"9016","output_tokens":17},"crc":"00000000"}"#;
        write_v2_log(
            &sessions,
            "vi5FAUzM30R7",
            &[V2_CREATED, "not json", bad_usage],
        );
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        assert_eq!(result.skipped.len(), 2, "{:?}", result.skipped);
        assert!(
            result.skipped[0].ends_with("log.jsonl:2"),
            "{}",
            result.skipped[0]
        );
        assert!(
            result.skipped[1].ends_with("log.jsonl:3"),
            "{}",
            result.skipped[1]
        );
        fs::remove_dir_all(&dir).ok();
    }
}

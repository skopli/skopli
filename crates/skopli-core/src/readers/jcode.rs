use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, as_string, dir_exists, file_mtime_iso, file_stem_name,
    finite_number, is_record, list_files, parse_timestamp_to_iso, read_json, read_jsonl_lines,
};
use crate::types::{TokenCounts, UsageEvent};

/// The jcode reader wired into the harness registry. jcode stores each session
/// as a snapshot `<sessionId>.json` under `~/.jcode/sessions/` plus an optional
/// append-only journal `<sessionId>.journal.jsonl` (an incremental event stream
/// over the snapshot). When a journal exists it is authoritative and the
/// snapshot's aggregates are skipped, so per-message usage is never double
/// counted; when no journal exists the snapshot is read directly. Corrupt/backup
/// sidecars (`.corrupt.jsonl`, `.pre-wipe-<ts>.bak`) are ignored, and the
/// `~/.jcode/ambient/` transcripts store is skipped (it carries no token-bearing
/// usage per the format research).
pub struct JcodeReader;

impl Reader for JcodeReader {
    fn harness_id(&self) -> &'static str {
        "jcode"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let home = jcode_home(ctx.env("JCODE_HOME"), ctx.home());
        read_jcode(&home)
    }
}

/// Resolve the jcode home. `JCODE_HOME` wins when set; otherwise `~/.jcode`.
pub fn jcode_home(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join(".jcode")
}

const UNKNOWN_MODEL: &str = "jcode-unknown";

/// Anthropic-style per-message usage, written as `usage` (journal) or
/// `token_usage` (snapshot). OpenAI-compatible providers also record
/// `prompt_tokens`, whose `input_tokens` twin includes the cached tokens, so
/// those are subtracted to count each token once. A message with no usage
/// object, or one whose token counts are all zero, yields `None`.
fn parse_usage(message: &Value) -> Option<TokenCounts> {
    let usage = match message.get("usage").or_else(|| message.get("token_usage")) {
        Some(u) if is_record(u) => u,
        _ => return None,
    };
    let pick = |key: &str| {
        usage
            .get(key)
            .and_then(finite_number)
            .unwrap_or(0.0)
            .max(0.0)
    };
    let output = pick("output_tokens");
    let cache_read = pick("cache_read_input_tokens");
    let cache_write = pick("cache_creation_input_tokens");
    // Snapshots write `prompt_tokens` as the cache-inclusive context total for every
    // provider, while `input_tokens` is provider-native (inclusive for OpenAI-compatible
    // wires, uncached for split-accounting wires), so the total is the stable base.
    let input = if usage.get("prompt_tokens").is_some() {
        (pick("prompt_tokens") - cache_read - cache_write).max(0.0)
    } else {
        pick("input_tokens")
    };
    if input == 0.0 && output == 0.0 && cache_read == 0.0 && cache_write == 0.0 {
        return None;
    }
    Some(TokenCounts {
        input: input as u64,
        output: output as u64,
        cache_read: cache_read as u64,
        cache_write: cache_write as u64,
        cache_write1h: None,
        reasoning: 0,
    })
}

/// Build an event from one message (snapshot entry or journal payload).
/// `session_model` is the snapshot-level `model`, used when the message has none.
fn event_from_message(
    message: &Value,
    session_id: &str,
    session_model: Option<&Value>,
    file: &str,
    index: usize,
    fallback_timestamp: &str,
) -> Option<UsageEvent> {
    let tokens = parse_usage(message)?;
    let timestamp = message
        .get("timestamp")
        .and_then(parse_timestamp_to_iso)
        .unwrap_or_else(|| fallback_timestamp.to_owned());
    let model = message
        .get("model")
        .or(session_model)
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| UNKNOWN_MODEL.to_owned());
    let message_id = message
        .get("id")
        .and_then(as_string)
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("{file}:{index}"));
    Some(UsageEvent {
        harness: "jcode".to_owned(),
        timestamp,
        session_id: session_id.to_owned(),
        message_id,
        turn: true,
        subagent: false,
        model,
        tokens,
        calls: None,
        cost_usd: None,
        workspace: None,
        title: None,
    })
}

/// Read events from a journal file: each line is `{type:"message", message:{...}}`
/// (the incremental event stream). Non-message lines are ignored.
fn read_journal(
    file: &str,
    session_id: &str,
    fallback_timestamp: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let mut events = Vec::new();
    let lines = match read_jsonl_lines(file, skipped, warnings) {
        Some(l) => l,
        None => return events,
    };
    for JsonlLine { index, value } in &lines {
        if !is_record(value) {
            continue;
        }
        if value.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let message = match value.get("message") {
            Some(m) if is_record(m) => m,
            _ => continue,
        };
        if let Some(event) =
            event_from_message(message, session_id, None, file, *index, fallback_timestamp)
        {
            events.push(event);
        }
    }
    events
}

/// The session id declared in a parsed snapshot, falling back to the file stem.
fn snapshot_session_id(parsed: &Value, file: &str) -> String {
    parsed
        .get("session_id")
        .and_then(as_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| file_stem_name(file))
}

/// Read events from a parsed snapshot: `{session_id?, messages:[{...}]}`.
fn read_snapshot(
    parsed: &Value,
    session_id: &str,
    file: &str,
    fallback_timestamp: &str,
) -> Vec<UsageEvent> {
    let messages = match parsed.get("messages") {
        Some(Value::Array(a)) => a,
        _ => return Vec::new(),
    };
    let mut events = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if !is_record(message) {
            continue;
        }
        if let Some(event) = event_from_message(
            message,
            session_id,
            parsed.get("model"),
            file,
            index,
            fallback_timestamp,
        ) {
            events.push(event);
        }
    }
    events
}

/// Read all jcode usage events. For each snapshot in `<home>/sessions/`, prefer
/// its journal (`<sessionId>.journal.jsonl`) when present, else read the
/// snapshot itself. Sidecars are excluded by the file predicate; the ambient
/// store is never walked.
pub fn read_jcode(home: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let sessions = home.join("sessions");
    if !dir_exists(&sessions) {
        return ReaderResult {
            events,
            skipped,
            warnings,
        };
    }

    let snapshots = list_files(&sessions, |name| {
        name.ends_with(".json") && !name.ends_with(".journal.jsonl")
    });
    for snapshot in &snapshots {
        let parsed = match read_json(snapshot) {
            Some(v) if is_record(&v) => v,
            _ => continue,
        };
        let session_id = snapshot_session_id(&parsed, snapshot);
        let journal = journal_path(snapshot);
        let fallback_timestamp = file_mtime_iso(snapshot);
        if Path::new(&journal).is_file() {
            events.extend(read_journal(
                &journal,
                &session_id,
                &fallback_timestamp,
                &mut skipped,
                &mut warnings,
            ));
        } else {
            events.extend(read_snapshot(
                &parsed,
                &session_id,
                snapshot,
                &fallback_timestamp,
            ));
        }
    }

    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

/// The journal path next to a snapshot: `<sessionId>.json` -> `<sessionId>.journal.jsonl`.
fn journal_path(snapshot: &str) -> String {
    let trimmed = snapshot.strip_suffix(".json").unwrap_or(snapshot);
    format!("{trimmed}.journal.jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readers::shared::{scratch_dir, test_home};
    use std::fs;

    #[test]
    fn default_home_resolves_dot_jcode() {
        assert_eq!(jcode_home(None, &test_home()), test_home().join(".jcode"));
    }

    #[test]
    fn jcode_home_override_wins() {
        assert_eq!(
            jcode_home(Some("/custom/jcode"), &test_home()),
            PathBuf::from("/custom/jcode")
        );
    }

    #[test]
    fn empty_override_falls_through_to_default() {
        assert_eq!(
            jcode_home(Some(""), &test_home()),
            test_home().join(".jcode")
        );
    }

    #[test]
    fn empty_home_yields_no_events_then_reads_after_sessions_appear() {
        let dir = scratch_dir("presence");
        let home = dir.join(".jcode");
        // Absence: the sessions dir does not exist yet.
        assert!(read_jcode(&home).events.is_empty());
        // Presence: a snapshot appears and is read.
        let sessions = home.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("s1.json"),
            r#"{"session_id":"s1","messages":[{"id":"m1","timestamp":"2026-08-01T10:00:00.000Z","model":"jcode-x","usage":{"input_tokens":5,"output_tokens":7}}]}"#,
        )
        .unwrap();
        let events = read_jcode(&home).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message_id, "m1");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn split_accounting_snapshot_derives_uncached_input_from_the_total() {
        // Anthropic-wire snapshots keep `input_tokens` at the uncached count and
        // `prompt_tokens` at input + cache read + cache creation.
        let dir = scratch_dir("token-usage-split");
        let sessions = dir.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("session_b.json"),
            r#"{"id":"session_b","model":"m","messages":[{"id":"a1","role":"assistant","content":[],"timestamp":"2026-10-25T12:30:00Z","token_usage":{"prompt_tokens":15698,"input_tokens":1,"output_tokens":25,"cache_read_input_tokens":7111,"cache_creation_input_tokens":8586}}]}"#,
        )
        .unwrap();
        let e = read_jcode(&dir).events.remove(0);
        assert_eq!(
            (
                e.tokens.input,
                e.tokens.output,
                e.tokens.cache_read,
                e.tokens.cache_write
            ),
            (1, 25, 7111, 8586)
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn snapshot_token_usage_and_session_model() {
        // Current snapshots write per-message `token_usage` (OpenAI-compatible
        // providers include cached tokens in input) and keep the model on the
        // session, not on the message.
        let dir = scratch_dir("token-usage");
        let sessions = dir.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("session_a.json"),
            r#"{"id":"session_a","model":"qwen","provider_key":"openai-compatible","messages":[{"id":"u1","role":"user","content":[],"timestamp":"2026-10-25T12:10:44.555Z"},{"id":"a1","role":"assistant","content":[],"timestamp":"2026-10-25T12:13:07.738265960Z","token_usage":{"prompt_tokens":15696,"input_tokens":15696,"output_tokens":54,"cache_read_input_tokens":3}}]}"#,
        )
        .unwrap();
        let events = read_jcode(&dir).events;
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!((e.message_id.as_str(), e.model.as_str()), ("a1", "qwen"));
        assert_eq!(e.timestamp, "2026-10-25T12:13:07.738Z");
        assert_eq!(
            (e.tokens.input, e.tokens.output, e.tokens.cache_read),
            (15693, 54, 3)
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn journal_supersedes_snapshot_aggregates() {
        let dir = scratch_dir("journal");
        let sessions = dir.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("s1.json"),
            r#"{"session_id":"s1","messages":[{"id":"stale","timestamp":"2026-08-01T09:00:00.000Z","model":"jcode-x","usage":{"input_tokens":9999,"output_tokens":9999}}]}"#,
        )
        .unwrap();
        fs::write(
            sessions.join("s1.journal.jsonl"),
            "{\"type\":\"message\",\"message\":{\"id\":\"j1\",\"timestamp\":\"2026-08-01T10:00:00.000Z\",\"model\":\"jcode-x\",\"usage\":{\"input_tokens\":10,\"output_tokens\":20}}}\n",
        )
        .unwrap();
        let events = read_jcode(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message_id, "j1");
        assert_eq!(events[0].tokens.input, 10);
        assert_eq!(events[0].tokens.output, 20);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sidecars_are_ignored() {
        let dir = scratch_dir("sidecars");
        let sessions = dir.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("s1.corrupt.jsonl"),
            "{\"type\":\"message\",\"message\":{\"id\":\"c\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n",
        )
        .unwrap();
        fs::write(
            sessions.join("s1.pre-wipe-123.bak"),
            r#"{"session_id":"s1","messages":[{"id":"b","usage":{"input_tokens":1,"output_tokens":1}}]}"#,
        )
        .unwrap();
        assert!(read_jcode(&dir).events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }
}

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, as_string, dir_exists, file_mtime_iso, file_stem_name,
    finite_number, is_record, list_files, parse_timestamp_to_iso, read_jsonl_lines,
};
use crate::types::{TokenCounts, UsageEvent};

/// The OpenCodeReview reader wired into the harness registry. OpenCodeReview
/// (Alibaba `ocr`) writes one JSONL file per review session under
/// `<home>/.opencodereview/sessions/<encoded-repo-path>/<session-id>.jsonl`.
/// Each `llm_response` line carries its own `usage {prompt_tokens,
/// completion_tokens, cache_read_tokens, cache_write_tokens}` and `model`, and
/// the tool builds its own totals by summing those per-response values. The
/// run-level `session_end` `summary` is a rollup of the same numbers, so the
/// reader emits one event per `llm_response` and never reads the summary,
/// eliminating any double count.
pub struct OpencodereviewReader;

impl Reader for OpencodereviewReader {
    fn harness_id(&self) -> &'static str {
        "opencodereview"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        read_opencodereview(ctx.home())
    }
}

const HARNESS: &str = "opencodereview";
const UNKNOWN_MODEL: &str = "opencodereview-unknown";

/// The sessions root: `<home>/.opencodereview/sessions`.
pub fn opencodereview_sessions_root(home: &Path) -> PathBuf {
    home.join(".opencodereview").join("sessions")
}

/// An optional token cell (`cache_read_tokens`/`cache_write_tokens` are
/// `omitempty` upstream). An absent or JSON-`null` cell is `0`; a populated
/// cell that is not a non-negative finite number is invalid (`None`), which
/// marks the whole record malformed. Mirrors the null-vs-populated-invalid
/// split used by the cherrystudio reader.
fn optional_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// A required token cell (`prompt_tokens`/`completion_tokens` are always
/// written upstream). The cell must be populated with a non-negative finite
/// number; an absent, `null`, or invalid cell marks the record malformed.
fn required_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// The outcome of parsing one `llm_response` usage map.
enum UsageOutcome {
    /// A populated, valid usage map with a non-zero total.
    Counts(TokenCounts),
    /// A valid usage map whose token counts are all zero (dropped, not warned).
    Zero,
    /// The usage map is absent or not an object, or a populated cell is invalid.
    Malformed,
}

/// Per-`llm_response` usage. A record whose `usage` is absent or not an object,
/// whose required `prompt_tokens`/`completion_tokens` are absent or invalid, or
/// one of whose populated optional cells is not a non-negative number, is
/// `Malformed`. An all-zero usage map is `Zero`.
fn parse_usage(record: &Value) -> UsageOutcome {
    let usage = match record.get("usage") {
        Some(u) if is_record(u) => u,
        _ => return UsageOutcome::Malformed,
    };
    let cells = [
        required_cell(usage.get("prompt_tokens")),
        required_cell(usage.get("completion_tokens")),
        optional_cell(usage.get("cache_read_tokens")),
        optional_cell(usage.get("cache_write_tokens")),
    ];
    let [input, output, cache_read, cache_write] = match cells {
        [Some(i), Some(o), Some(cr), Some(cw)] => [i, o, cr, cw],
        _ => return UsageOutcome::Malformed,
    };
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return UsageOutcome::Zero;
    }
    UsageOutcome::Counts(TokenCounts {
        input,
        output,
        cache_read,
        cache_write,
        cache_write1h: None,
        reasoning: 0,
    })
}

/// The session-level fields captured from the leading `session_start` record:
/// its RFC-3339 `timestamp` and its declared `model` (used when an
/// `llm_response` omits its own model).
#[derive(Default)]
struct SessionHeader {
    timestamp: Option<String>,
    model: Option<String>,
}

/// Read one review-session JSONL file into usage events. The session id is the
/// file stem; the session timestamp comes from the `session_start` record (or
/// the file mtime when absent), since `llm_response` lines carry no per-line
/// timestamp.
fn read_session(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let mut events: Vec<UsageEvent> = Vec::new();
    let lines = match read_jsonl_lines(file, skipped, warnings) {
        Some(l) => l,
        None => return events,
    };

    let session_id = file_stem_name(file);
    let fallback_timestamp = file_mtime_iso(file);
    let mut header = SessionHeader::default();

    for JsonlLine { index, value } in &lines {
        if !is_record(value) {
            continue;
        }
        let record_type = value.get("type").and_then(Value::as_str);
        if record_type == Some("session_start") {
            if header.timestamp.is_none()
                && let Some(ts) = value.get("timestamp").and_then(parse_timestamp_to_iso)
            {
                header.timestamp = Some(ts);
            }
            if header.model.is_none()
                && let Some(model) = value
                    .get("model")
                    .and_then(as_string)
                    .filter(|m| !m.trim().is_empty())
            {
                header.model = Some(model);
            }
            continue;
        }
        if record_type != Some("llm_response") {
            continue;
        }
        let tokens = match parse_usage(value) {
            UsageOutcome::Counts(t) => t,
            UsageOutcome::Zero => continue,
            UsageOutcome::Malformed => {
                let loc = format!("{file}:{}", index + 1);
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed opencodereview record {loc}\n"),
                });
                skipped.push(loc);
                continue;
            }
        };
        let model = value
            .get("model")
            .and_then(as_string)
            .filter(|m| !m.trim().is_empty())
            .or_else(|| header.model.clone())
            .unwrap_or_else(|| UNKNOWN_MODEL.to_owned());
        let timestamp = header
            .timestamp
            .clone()
            .unwrap_or_else(|| fallback_timestamp.clone());
        events.push(UsageEvent {
            harness: HARNESS.to_owned(),
            timestamp,
            session_id: session_id.clone(),
            message_id: format!("{file}:{index}"),
            turn: true,
            subagent: false,
            model,
            tokens,
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        });
    }
    events
}

/// Read all OpenCodeReview usage events reachable from `home`. Each
/// `<sessions>/<encoded-repo>/<session>.jsonl` is summed per `llm_response`.
pub fn read_opencodereview(home: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let sessions = opencodereview_sessions_root(home);
    if !dir_exists(&sessions) {
        return ReaderResult {
            events,
            skipped,
            warnings,
        };
    }

    let files = list_files(&sessions, |name| name.ends_with(".jsonl"));
    for file in &files {
        events.extend(read_session(file, &mut skipped, &mut warnings));
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
    use std::fs;

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-ocr-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn write_session(dir: &Path, repo: &str, name: &str, body: &str) {
        let repo_dir = dir.join(".opencodereview").join("sessions").join(repo);
        fs::create_dir_all(&repo_dir).unwrap();
        fs::write(repo_dir.join(name), body).unwrap();
    }

    #[test]
    fn sessions_root_resolves_under_home() {
        let home = PathBuf::from("/home/u");
        assert_eq!(
            opencodereview_sessions_root(&home),
            home.join(".opencodereview").join("sessions")
        );
    }

    #[test]
    fn empty_home_yields_no_events_then_reads_after_sessions_appear() {
        let dir = tmp_dir("presence");
        assert!(read_opencodereview(&dir).events.is_empty());
        write_session(
            &dir,
            "-home-user-proj",
            "sess-a.jsonl",
            "{\"type\":\"session_start\",\"timestamp\":\"2026-08-01T10:00:00Z\",\"model\":\"ocr-model\"}\n{\"type\":\"llm_response\",\"model\":\"ocr-model\",\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n",
        );
        let events = read_opencodereview(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, "sess-a");
        assert_eq!(events[0].model, "ocr-model");
        assert_eq!(events[0].timestamp, "2026-08-01T10:00:00.000Z");
        assert_eq!(events[0].tokens.input, 10);
        assert_eq!(events[0].tokens.output, 5);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sums_each_llm_response_and_ignores_session_end_summary() {
        let dir = tmp_dir("sum");
        write_session(
            &dir,
            "-home-user-proj",
            "sess-b.jsonl",
            "{\"type\":\"session_start\",\"timestamp\":\"2026-08-02T09:00:00Z\",\"model\":\"m1\"}\n\
             {\"type\":\"llm_response\",\"model\":\"m1\",\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50,\"cache_read_tokens\":30,\"cache_write_tokens\":20}}\n\
             {\"type\":\"llm_response\",\"model\":\"m2\",\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":9}}\n\
             {\"type\":\"session_end\",\"summary\":{\"total_tokens\":9999,\"input_tokens\":9999,\"output_tokens\":9999,\"cache_read_tokens\":9999,\"cache_write_tokens\":9999}}\n",
        );
        let events = read_opencodereview(&dir).events;
        assert_eq!(
            events.len(),
            2,
            "one event per llm_response, summary ignored"
        );
        assert_eq!(events[0].model, "m1");
        assert_eq!(events[0].tokens.cache_read, 30);
        assert_eq!(events[0].tokens.cache_write, 20);
        assert_eq!(events[1].model, "m2");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn llm_response_without_model_falls_back_to_session_model() {
        let dir = tmp_dir("model-fallback");
        write_session(
            &dir,
            "-home-user-proj",
            "sess-c.jsonl",
            "{\"type\":\"session_start\",\"timestamp\":\"2026-08-03T09:00:00Z\",\"model\":\"session-model\"}\n{\"type\":\"llm_response\",\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n",
        );
        let events = read_opencodereview(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "session-model");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zero_usage_lines_are_dropped() {
        let dir = tmp_dir("zero");
        write_session(
            &dir,
            "-home-user-proj",
            "sess-d.jsonl",
            "{\"type\":\"session_start\",\"timestamp\":\"2026-08-04T09:00:00Z\",\"model\":\"m\"}\n{\"type\":\"llm_response\",\"model\":\"m\",\"usage\":{\"prompt_tokens\":0,\"completion_tokens\":0}}\n",
        );
        assert!(read_opencodereview(&dir).events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn null_optional_cells_stay_zero_but_populated_invalid_cells_are_skipped() {
        let dir = tmp_dir("malformed");
        write_session(
            &dir,
            "-home-user-proj",
            "sess-e.jsonl",
            "{\"type\":\"session_start\",\"timestamp\":\"2026-08-05T09:00:00Z\",\"model\":\"m\"}\n\
             {\"type\":\"llm_response\",\"model\":\"m\",\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"cache_read_tokens\":null,\"cache_write_tokens\":null}}\n\
             {\"type\":\"llm_response\",\"model\":\"m\",\"usage\":{\"prompt_tokens\":\"oops\",\"completion_tokens\":5}}\n",
        );
        let file = dir
            .join(".opencodereview")
            .join("sessions")
            .join("-home-user-proj")
            .join("sess-e.jsonl")
            .to_string_lossy()
            .into_owned();
        let result = read_opencodereview(&dir);
        assert_eq!(
            result.events.len(),
            1,
            "null cells map to 0; invalid row skipped"
        );
        assert_eq!(result.events[0].tokens.input, 10);
        assert_eq!(result.events[0].tokens.cache_read, 0);
        assert_eq!(result.skipped, vec![format!("{file}:3")]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed opencodereview record {file}:3\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absent_or_null_required_cells_skip_the_record() {
        let dir = tmp_dir("required");
        write_session(
            &dir,
            "-home-user-proj",
            "sess-f.jsonl",
            "{\"type\":\"session_start\",\"timestamp\":\"2026-08-06T09:00:00Z\",\"model\":\"m\"}\n\
             {\"type\":\"llm_response\",\"model\":\"m\",\"usage\":{\"completion_tokens\":5}}\n\
             {\"type\":\"llm_response\",\"model\":\"m\",\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":null}}\n\
             {\"type\":\"llm_response\",\"model\":\"m\",\"usage\":{\"cache_read_tokens\":7}}\n",
        );
        let file = dir
            .join(".opencodereview")
            .join("sessions")
            .join("-home-user-proj")
            .join("sess-f.jsonl")
            .to_string_lossy()
            .into_owned();
        let result = read_opencodereview(&dir);
        assert!(
            result.events.is_empty(),
            "no partial events from missing required cells"
        );
        assert_eq!(
            result.skipped,
            vec![
                format!("{file}:2"),
                format!("{file}:3"),
                format!("{file}:4")
            ]
        );
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed opencodereview record {file}:2\n")
        );
        fs::remove_dir_all(&dir).ok();
    }
}

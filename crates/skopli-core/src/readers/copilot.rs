use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, ScanJsonl, as_string, date_parse_ms, dir_exists,
    file_mtime_iso, finite_number, is_record, list_files, optional_cell, parent_dir_name,
    read_jsonl_lines, required_cell, scan_jsonl,
};
use crate::time::to_iso_string;
use crate::types::{TokenCounts, UsageEvent};

/// Copilot CLI records usage in two places: the always-on session state
/// (`~/.copilot/session-state/<id>/events.jsonl`, one `session.usage_record`
/// per model request) and the opt-in OTel file exporter. Session state owns
/// every session it records; OTel events are kept only for sessions the
/// session state does not know, so a request is never counted twice.
pub struct CopilotReader;

impl Reader for CopilotReader {
    fn harness_id(&self) -> &'static str {
        "copilot"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let mut result = read_copilot_session_state(&copilot_session_state_root(ctx.home()));
        let owned: HashSet<&str> = result
            .events
            .iter()
            .map(|e| e.session_id.as_str())
            .collect();
        let otel = read_copilot_otel(&copilot_otel_root(
            ctx.env("COPILOT_OTEL_FILE_EXPORTER_PATH"),
            ctx.home(),
        ));
        let mut otel_events: Vec<UsageEvent> = otel
            .events
            .into_iter()
            .filter(|e| !owned.contains(e.session_id.as_str()))
            .collect();
        result.events.append(&mut otel_events);
        result.skipped.extend(otel.skipped);
        result.warnings.extend(otel.warnings);
        result
    }
}

pub fn copilot_session_state_root(home: &Path) -> PathBuf {
    home.join(".copilot").join("session-state")
}

/// Read `events.jsonl` session logs under the session-state root. The
/// `session.start` record names the session and its working directory; each
/// `session.usage_record` carries one request's model and token counts.
pub fn read_copilot_session_state(root: &Path) -> ReaderResult {
    let mut result = ReaderResult::default();
    for file in list_files(root, |name| name == "events.jsonl") {
        let Some(lines) = read_jsonl_lines(&file, &mut result.skipped, &mut result.warnings) else {
            continue;
        };
        let mut session = SessionHeader::default();
        for line in lines {
            match line.value.get("type").and_then(Value::as_str) {
                Some("session.start") => {
                    if let Some(data) = line.value.get("data") {
                        session = SessionHeader::parse(data);
                    }
                }
                Some("session.usage_record") => {
                    let session_id = session.id.clone().unwrap_or_else(|| parent_dir_name(&file));
                    match usage_record_event(&line.value, &session_id, session.workspace.clone()) {
                        Some(event) => result.events.push(event),
                        None => {
                            let location = format!("{file}:{}", line.index + 1);
                            result.skipped.push(location.clone());
                            result.warnings.push(ReaderWarning {
                                message: format!(
                                    "skipping malformed copilot usage record {location}\n"
                                ),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
    result
}

#[derive(Default)]
struct SessionHeader {
    id: Option<String>,
    workspace: Option<String>,
}

impl SessionHeader {
    fn parse(data: &Value) -> Self {
        SessionHeader {
            id: data.get("sessionId").and_then(as_string),
            workspace: data
                .get("context")
                .and_then(|c| c.get("cwd"))
                .and_then(as_string),
        }
    }
}

fn usage_record_event(
    record: &Value,
    session_id: &str,
    workspace: Option<String>,
) -> Option<UsageEvent> {
    let usage = record.get("data")?.get("usage")?;
    let timestamp = record
        .get("timestamp")
        .and_then(as_string)
        .and_then(|t| date_parse_ms(t.trim()))
        .and_then(to_iso_string)?;
    Some(UsageEvent {
        harness: "copilot".to_owned(),
        timestamp,
        session_id: session_id.to_owned(),
        message_id: format!("{session_id}:{}", record.get("id").and_then(as_string)?),
        turn: false,
        subagent: false,
        model: usage.get("model").and_then(as_string)?,
        tokens: TokenCounts {
            input: required_cell(usage.get("inputTokens"))?,
            output: required_cell(usage.get("outputTokens"))?,
            cache_read: optional_cell(usage.get("cacheReadTokens"))?,
            cache_write: optional_cell(usage.get("cacheWriteTokens"))?,
            cache_write1h: None,
            reasoning: 0,
        },
        calls: None,
        // `usage.cost` is the premium-request multiplier, not a dollar amount.
        cost_usd: None,
        workspace,
        title: None,
    })
}

/// Resolve the copilot OTel root.
pub fn copilot_otel_root(config: Option<&str>, home: &Path) -> PathBuf {
    if let Some(override_val) = config
        && !override_val.is_empty()
    {
        return PathBuf::from(override_val);
    }
    home.join(".copilot").join("otel")
}

fn attribute_number(attributes: &Value, names: &[&str]) -> Option<f64> {
    for name in names {
        let value = attributes.get(*name);
        if let Some(v) = value {
            if let Some(direct) = finite_number(v) {
                return Some(direct);
            }
            if is_record(v)
                && let Some(nested) = v.get("value").and_then(finite_number)
            {
                return Some(nested);
            }
        }
    }
    None
}

fn attribute_string(attributes: &Value, names: &[&str]) -> Option<String> {
    for name in names {
        let value = attributes.get(*name);
        if let Some(v) = value {
            if let Some(direct) = as_string(v)
                && !direct.is_empty()
            {
                return Some(direct);
            }
            if is_record(v)
                && let Some(nested) = v.get("value").and_then(as_string)
                && !nested.is_empty()
            {
                return Some(nested);
            }
        }
    }
    None
}

fn all_zeros(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b == b'0')
}

fn identity(raw: &Value, name: &str) -> Option<String> {
    if let Some(direct) = raw.get(name).and_then(as_string)
        && !direct.is_empty()
        && !all_zeros(&direct)
    {
        return Some(direct);
    }
    let context = raw.get("spanContext");
    let context = match context {
        Some(c) if is_record(c) => c,
        _ => return None,
    };
    match context.get(name).and_then(as_string) {
        Some(nested) if !nested.is_empty() && !all_zeros(&nested) => Some(nested),
        _ => None,
    }
}

/// The first parseable of startTime/timestamp/time re-serialized as ISO, else
/// the fallback.
fn timestamp(raw: &Value, fallback: &str) -> String {
    ["startTime", "timestamp", "time"]
        .into_iter()
        .filter_map(|name| raw.get(name))
        .find_map(epoch_ms)
        .and_then(to_iso_string)
        .unwrap_or_else(|| fallback.to_owned())
}

/// An ISO 8601 string or the OTel JS SDK's hrtime tuple `[seconds, nanos]`
/// (the file exporter's span time shape) as epoch milliseconds.
fn epoch_ms(value: &Value) -> Option<i64> {
    match value {
        Value::String(s) => date_parse_ms(s.trim()),
        Value::Array(parts) => match parts.as_slice() {
            [seconds, nanos] => seconds
                .as_i64()?
                .checked_mul(1000)?
                .checked_add(nanos.as_i64()? / 1_000_000),
            _ => None,
        },
        _ => None,
    }
}

/// A parsed OTel usage candidate before source-priority selection.
struct Candidate {
    event: UsageEvent,
    source: i32,
    trace_id: String,
    response_id: Option<String>,
    inclusive_input: f64,
}

/// Read all copilot usage events from the given root.
pub fn read_copilot_otel(root: &Path) -> ReaderResult {
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let root_str = root.to_string_lossy().into_owned();
    if !root.exists() {
        return ReaderResult::default();
    }
    let files: Vec<String> = if dir_exists(root) && !root_str.ends_with(".jsonl") {
        list_files(root, |name| name.ends_with(".jsonl"))
    } else {
        vec![root_str]
    };

    // scan_jsonl's parse closure needs to push skip diagnostics; collect them
    // into a side channel and merge after.
    let mut inner_skipped: Vec<String> = Vec::new();
    let mut inner_warnings: Vec<ReaderWarning> = Vec::new();
    let mut parse = |line: &JsonlLine, file: &str| -> Option<Candidate> {
        parse_candidate(line, file, &mut inner_skipped, &mut inner_warnings)
    };
    let values = scan_jsonl(
        ScanJsonl {
            files: &files,
            parse: &mut parse,
            dedup_key: None,
        },
        &mut skipped,
        &mut warnings,
    );
    skipped.extend(inner_skipped);
    warnings.extend(inner_warnings);

    // Source-priority selection + same-source max merge.
    let mut selected: Vec<Candidate> = Vec::new();
    let mut index_by_id: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for candidate in values.iter() {
        let suppressed = values.iter().any(|other| {
            other.source < candidate.source
                && (other.trace_id == candidate.trace_id
                    || (candidate.response_id.is_some()
                        && other.response_id == candidate.response_id))
        });
        if suppressed {
            continue;
        }
        let msg_id = candidate.event.message_id.clone();
        match index_by_id.get(&msg_id).copied() {
            None => {
                index_by_id.insert(msg_id, selected.len());
                selected.push(clone_candidate(candidate));
            }
            Some(idx) => {
                if candidate.source < selected[idx].source {
                    selected[idx] = clone_candidate(candidate);
                } else if candidate.source == selected[idx].source {
                    let prior = &mut selected[idx];
                    prior.inclusive_input = prior.inclusive_input.max(candidate.inclusive_input);
                    prior.event.tokens.output =
                        prior.event.tokens.output.max(candidate.event.tokens.output);
                    prior.event.tokens.cache_read = prior
                        .event
                        .tokens
                        .cache_read
                        .max(candidate.event.tokens.cache_read);
                    prior.event.tokens.cache_write = prior
                        .event
                        .tokens
                        .cache_write
                        .max(candidate.event.tokens.cache_write);
                    prior.event.tokens.reasoning = prior
                        .event
                        .tokens
                        .reasoning
                        .max(candidate.event.tokens.reasoning);
                    prior.event.tokens.input = (prior.inclusive_input
                        - prior.event.tokens.cache_read as f64)
                        .max(0.0) as u64;
                }
            }
        }
    }

    let events: Vec<UsageEvent> = selected.into_iter().map(|c| c.event).collect();
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

fn clone_candidate(c: &Candidate) -> Candidate {
    Candidate {
        event: c.event.clone(),
        source: c.source,
        trace_id: c.trace_id.clone(),
        response_id: c.response_id.clone(),
        inclusive_input: c.inclusive_input,
    }
}

/// Parse one OTel line into a usage candidate.
fn parse_candidate(
    line: &JsonlLine,
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Option<Candidate> {
    let value = &line.value;
    let index = line.index;
    if !is_record(value) {
        return None;
    }
    let attributes = value.get("attributes");
    let attributes = match attributes {
        Some(a) if is_record(a) => a,
        _ => return None,
    };
    let has_usage = attributes
        .as_object()
        .map(|m| m.keys().any(|k| k.starts_with("gen_ai.usage.")))
        .unwrap_or(false);
    if !has_usage {
        return None;
    }
    let input = attribute_number(attributes, &["gen_ai.usage.input_tokens"]);
    let output = attribute_number(attributes, &["gen_ai.usage.output_tokens"]);
    let (input, output) = match (input, output) {
        (Some(i), Some(o)) => (i, o),
        _ => {
            let location = format!("{file}:{}", index + 1);
            skipped.push(location.clone());
            warnings.push(ReaderWarning {
                message: format!("skipping malformed copilot OTel usage record {location}\n"),
            });
            return None;
        }
    };
    let trace_id = identity(value, "traceId");
    let span_id = identity(value, "spanId");
    let model = attribute_string(
        attributes,
        &["gen_ai.response.model", "gen_ai.request.model"],
    );
    let (trace_id, span_id, model) = match (trace_id, span_id, model) {
        (Some(t), Some(s), Some(m)) => (t, s, m),
        _ => {
            let location = format!("{file}:{}", index + 1);
            skipped.push(location.clone());
            warnings.push(ReaderWarning {
                message: format!("skipping malformed copilot OTel usage record {location}\n"),
            });
            return None;
        }
    };
    let name = value.get("name").and_then(as_string).unwrap_or_default();
    let event_name = attribute_string(attributes, &["event.name"]);
    let body = value.get("body").and_then(as_string).unwrap_or_default();
    let span = value.get("type").and_then(Value::as_str) == Some("span") || !name.is_empty();
    let op_name = attribute_string(attributes, &["gen_ai.operation.name"]);
    let source = if span && (op_name.as_deref() == Some("chat") || name.starts_with("chat ")) {
        0
    } else if !span
        && (event_name.as_deref() == Some("gen_ai.client.inference.operation.details")
            || body.starts_with("GenAI inference:"))
    {
        1
    } else if !span
        && (event_name.as_deref() == Some("copilot_chat.agent.turn")
            || body.starts_with("copilot_chat.agent.turn"))
    {
        2
    } else if span
        && (op_name.as_deref() == Some("invoke_agent") || name.starts_with("invoke_agent "))
    {
        3
    } else {
        -1
    };
    if source < 0 {
        return None;
    }

    let cache_read = attribute_number(
        attributes,
        &[
            "gen_ai.usage.cache_read.input_tokens",
            "gen_ai.usage.cache_read_input_tokens",
        ],
    )
    .unwrap_or(0.0);
    let cache_write = attribute_number(
        attributes,
        &[
            "gen_ai.usage.cache_write.input_tokens",
            "gen_ai.usage.cache_creation.input_tokens",
            "gen_ai.usage.cache_write_input_tokens",
            "gen_ai.usage.cache_creation_input_tokens",
        ],
    )
    .unwrap_or(0.0);
    let reasoning = attribute_number(
        attributes,
        &[
            "gen_ai.usage.reasoning.output_tokens",
            "gen_ai.usage.reasoning_tokens",
        ],
    )
    .unwrap_or(0.0);

    let session_id = attribute_string(
        attributes,
        &[
            "gen_ai.conversation.id",
            "copilot_chat.session_id",
            "github.copilot.interaction_id",
            "gen_ai.response.id",
        ],
    )
    .unwrap_or_else(|| trace_id.clone());

    let event = UsageEvent {
        harness: "copilot".to_owned(),
        timestamp: timestamp(value, &file_mtime_iso(file)),
        session_id,
        message_id: format!("{trace_id}:{span_id}"),
        turn: false,
        subagent: false,
        model,
        tokens: TokenCounts {
            input: (input - cache_read).max(0.0) as u64,
            output: output.max(0.0) as u64,
            cache_read: cache_read.max(0.0) as u64,
            cache_write: cache_write.max(0.0) as u64,
            cache_write1h: None,
            reasoning: reasoning.max(0.0) as u64,
        },
        calls: None,
        cost_usd: None,
        workspace: None,
        title: None,
    };
    Some(Candidate {
        source,
        trace_id,
        response_id: attribute_string(attributes, &["gen_ai.response.id"]),
        inclusive_input: input.max(0.0),
        event,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readers::shared::scratch_dir;
    use std::fs;

    const SESSION_START: &str = r#"{"type":"session.start","data":{"sessionId":"sess-1","startTime":"2026-10-18T18:28:26.796Z","context":{"cwd":"/work/proj"}},"id":"rec-1","timestamp":"2026-10-18T18:28:26.796Z"}"#;
    const USAGE_RECORD: &str = r#"{"type":"session.usage_record","data":{"usage":{"model":"qwen","inputTokens":11468,"outputTokens":19,"cacheReadTokens":5,"cacheWriteTokens":0,"cost":0.0}},"id":"rec-2","timestamp":"2026-10-18T18:30:27.454Z","parentId":"rec-1"}"#;
    const OTEL_SPAN: &str = r#"{"type":"span","name":"chat qwen","traceId":"t1","spanId":"s1","startTime":"2026-10-18T18:30:00.000Z","attributes":{"gen_ai.operation.name":"chat","gen_ai.conversation.id":"sess-1","gen_ai.request.model":"qwen","gen_ai.usage.input_tokens":11473,"gen_ai.usage.output_tokens":19}}"#;

    fn write(home: &Path, rel: &str, body: &str) {
        let path = home.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn read(home: &Path) -> ReaderResult {
        let result = CopilotReader.read(&ReaderContext::new(home.to_path_buf()));
        fs::remove_dir_all(home).ok();
        result
    }

    #[test]
    fn session_state_usage_record_emits_one_event() {
        let home = scratch_dir("copilot-session-state");
        write(
            &home,
            ".copilot/session-state/sess-1/events.jsonl",
            &format!("{SESSION_START}\n{USAGE_RECORD}\n"),
        );
        let result = read(&home);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.events.len(), 1);
        let event = &result.events[0];
        assert_eq!(event.session_id, "sess-1");
        assert_eq!(event.message_id, "sess-1:rec-2");
        assert_eq!(event.timestamp, "2026-10-18T18:30:27.454Z");
        assert_eq!(event.model, "qwen");
        assert_eq!(event.tokens.input, 11468);
        assert_eq!(event.tokens.output, 19);
        assert_eq!(event.tokens.cache_read, 5);
        assert_eq!(event.workspace.as_deref(), Some("/work/proj"));
        assert_eq!(event.cost_usd, None);
    }

    #[test]
    fn session_state_owns_sessions_it_records_and_otel_fills_the_rest() {
        let home = scratch_dir("copilot-two-sources");
        write(
            &home,
            ".copilot/session-state/sess-1/events.jsonl",
            &format!("{SESSION_START}\n{USAGE_RECORD}\n"),
        );
        let other = OTEL_SPAN
            .replace("sess-1", "sess-2")
            .replace("\"t1\"", "\"t2\"");
        write(
            &home,
            ".copilot/otel/a.jsonl",
            &format!("{OTEL_SPAN}\n{other}\n"),
        );
        let events = read(&home).events;
        let ids: Vec<(&str, &str)> = events
            .iter()
            .map(|e| (e.session_id.as_str(), e.message_id.as_str()))
            .collect();
        assert_eq!(ids, vec![("sess-1", "sess-1:rec-2"), ("sess-2", "t2:s1")]);
    }

    #[test]
    fn otel_hrtime_start_time_is_the_event_timestamp() {
        let home = scratch_dir("copilot-otel-hrtime");
        write(
            &home,
            ".copilot/otel",
            r#"{"type":"span","name":"chat qwen","traceId":"t1","spanId":"s1","startTime":[1792350001,747000000],"endTime":[1792350126,755000000],"attributes":{"gen_ai.operation.name":"chat","gen_ai.request.model":"qwen","gen_ai.conversation.id":"sess-1","gen_ai.usage.input_tokens":11465,"gen_ai.usage.output_tokens":10,"gen_ai.usage.cache_read.input_tokens":5,"gen_ai.response.model":"qwen"}}
"#,
        );
        let result = read(&home);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].timestamp, "2026-10-18T19:00:01.747Z");
    }

    #[test]
    fn malformed_usage_records_are_a_diagnostic() {
        let cases = [
            ("no counts", r#""model":"qwen""#),
            (
                "negative input",
                r#""model":"qwen","inputTokens":-1,"outputTokens":19"#,
            ),
            (
                "negative cache read",
                r#""model":"qwen","inputTokens":1,"outputTokens":19,"cacheReadTokens":-5"#,
            ),
            (
                "string cache write",
                r#""model":"qwen","inputTokens":1,"outputTokens":19,"cacheWriteTokens":"0""#,
            ),
        ];
        for (case, usage) in cases {
            let home = scratch_dir("copilot-session-bad");
            write(
                &home,
                ".copilot/session-state/sess-1/events.jsonl",
                &format!(
                    r#"{{"type":"session.usage_record","data":{{"usage":{{{usage}}}}},"id":"rec-9","timestamp":"2026-10-18T18:30:27.454Z"}}"#
                ),
            );
            let result = read(&home);
            assert!(result.events.is_empty(), "{case}");
            assert_eq!(result.skipped.len(), 1, "{case}");
            assert!(result.skipped[0].ends_with("events.jsonl:1"), "{case}");
        }
    }
}

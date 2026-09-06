use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, ScanJsonl, as_string, dir_exists, file_mtime_iso,
    finite_number, is_record, list_files, scan_jsonl,
};
use crate::time::to_iso_string;
use crate::types::{TokenCounts, UsageEvent};

/// The copilot (OTel file exporter) reader wired into the harness registry.
pub struct CopilotReader;

impl Reader for CopilotReader {
    fn harness_id(&self) -> &'static str {
        "copilot"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = copilot_otel_root(ctx.env("COPILOT_OTEL_FILE_EXPORTER_PATH"), ctx.home());
        read_copilot_otel(&root)
    }
}

/// Resolve the copilot OTel root. Faithful port of `copilotOtelRoot`.
pub fn copilot_otel_root(config: Option<&str>, home: &Path) -> PathBuf {
    if let Some(override_val) = config
        && !override_val.is_empty()
    {
        return PathBuf::from(override_val);
    }
    home.join(".copilot").join("otel")
}

/// Faithful port of `attributeNumber`.
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

/// Faithful port of `attributeString`.
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

/// Faithful port of `identity`.
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

/// Faithful port of `timestamp`: the first parseable of startTime/timestamp/time
/// re-emitted via `new Date(value).toISOString()`, else the fallback.
fn timestamp(raw: &Value, fallback: &str) -> String {
    for name in ["startTime", "timestamp", "time"] {
        if let Some(value) = raw.get(name).and_then(as_string)
            && let Some(ms) = parse_date_ms(&value)
            && let Some(iso) = to_iso_string(ms)
        {
            return iso;
        }
    }
    fallback.to_owned()
}

/// Parse an ISO-8601 / RFC-3339 datetime string to epoch milliseconds, matching
/// the subset of `Date.parse` the copilot OTel records exercise (a full
/// date-time with `Z` or a numeric offset, optional fractional seconds).
/// Returns `None` when unparseable, mirroring `Number.isNaN(Date.parse(...))`.
fn parse_date_ms(value: &str) -> Option<i64> {
    let s = value.trim();
    let bytes = s.as_bytes();
    // YYYY-MM-DD[T ]HH:MM:SS(.fff)?(Z|+-HH:MM)
    if bytes.len() < 20 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    if bytes[4] != b'-' {
        return None;
    }
    let month: u32 = s.get(5..7)?.parse().ok()?;
    if bytes[7] != b'-' {
        return None;
    }
    let day: u32 = s.get(8..10)?.parse().ok()?;
    if bytes[10] != b'T' && bytes[10] != b' ' {
        return None;
    }
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    if bytes[13] != b':' {
        return None;
    }
    let minute: i64 = s.get(14..16)?.parse().ok()?;
    if bytes[16] != b':' {
        return None;
    }
    let second: i64 = s.get(17..19)?.parse().ok()?;
    let mut rest = &s[19..];
    let mut millis: i64 = 0;
    if let Some(stripped) = rest.strip_prefix('.') {
        let frac_end = stripped
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(stripped.len());
        let frac = &stripped[..frac_end];
        // JS Date uses millisecond precision: take up to 3 digits.
        let mut ms_str = frac.chars().take(3).collect::<String>();
        while ms_str.len() < 3 {
            ms_str.push('0');
        }
        millis = ms_str.parse().ok()?;
        rest = &stripped[frac_end..];
    }
    // Offset: Z, +HH:MM or -HH:MM
    let mut offset_min: i64 = 0;
    match rest.as_bytes().first() {
        Some(b'Z') => {}
        Some(b'+') | Some(b'-') => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let off = &rest[1..];
            let oh: i64 = off.get(0..2)?.parse().ok()?;
            let om: i64 = if off.len() >= 5 {
                off.get(3..5)?.parse().ok()?
            } else if off.len() >= 4 {
                off.get(2..4)?.parse().ok()?
            } else {
                0
            };
            offset_min = sign * (oh * 60 + om);
        }
        None => {}
        _ => return None,
    }

    let days = days_from_civil(year, month, day);
    let ms = days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000 + millis
        - offset_min * 60_000;
    Some(ms)
}

/// Howard Hinnant's `days_from_civil` (days since 1970-01-01).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let d = d as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A parsed OTel usage candidate before source-priority selection.
struct Candidate {
    event: UsageEvent,
    source: i32,
    trace_id: String,
    response_id: Option<String>,
    inclusive_input: f64,
}

/// Read all copilot usage events from the given root. Faithful port of
/// `readCopilotOtel`.
pub fn read_copilot_otel(root: &Path) -> ReaderResult {
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let root_str = root.to_string_lossy().into_owned();
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

    // Source-priority selection + same-source max merge. Faithful port of the
    // `selected` map logic.
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

/// The `parse` callback of `scanJsonl` in `readCopilotOtel`.
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

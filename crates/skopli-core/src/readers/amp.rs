use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, file_mtime_iso, finite_number, is_record, list_files,
    read_json,
};
use crate::time::to_iso_string;
use crate::types::{TokenCounts, UsageEvent};

/// The amp reader wired into the harness registry.
pub struct AmpReader;

impl Reader for AmpReader {
    fn harness_id(&self) -> &'static str {
        "amp"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = amp_threads_root(ctx.env("AMP_DATA_DIR"), ctx.home());
        read_amp(&root)
    }
}

/// Resolve the amp threads root. Faithful port of `ampThreadsRoot`.
pub fn amp_threads_root(config: Option<&str>, home: &Path) -> PathBuf {
    if let Some(override_val) = config
        && !override_val.is_empty()
    {
        return PathBuf::from(override_val).join("threads");
    }
    home.join(".local")
        .join("share")
        .join("amp")
        .join("threads")
}

struct AmpRecord {
    event: UsageEvent,
    message_id: Option<String>,
    turn: bool,
}

/// Faithful port of `eventsId`.
fn events_id(usage: &Value) -> String {
    usage
        .get("id")
        .and_then(as_string)
        .unwrap_or_else(|| "ledger".to_owned())
}

/// Faithful port of `ampRecord`.
#[allow(clippy::too_many_arguments)]
fn amp_record(
    usage: &Value,
    session_id: &str,
    message_id: Option<String>,
    turn: bool,
    fallback_timestamp: &str,
    created: Option<f64>,
    numeric_message_id: Option<f64>,
    fallback_model: Option<String>,
) -> Option<AmpRecord> {
    let model = usage.get("model").and_then(as_string).or(fallback_model);
    let input = usage
        .get("inputTokens")
        .and_then(finite_number)
        .or_else(|| usage.get("input").and_then(finite_number));
    let output = usage
        .get("outputTokens")
        .and_then(finite_number)
        .or_else(|| usage.get("output").and_then(finite_number));
    let (model, input, output) = match (model, input, output) {
        (Some(m), Some(i), Some(o)) => (m, i, o),
        _ => return None,
    };
    let timestamp = usage
        .get("timestamp")
        .and_then(as_string)
        .unwrap_or_else(|| match (created, numeric_message_id) {
            (Some(c), Some(n)) => to_iso_string((c + n * 1000.0) as i64).unwrap_or_default(),
            _ => fallback_timestamp.to_owned(),
        });
    let resolved_message_id = message_id
        .clone()
        .unwrap_or_else(|| format!("{session_id}:{}", events_id(usage)));
    Some(AmpRecord {
        message_id,
        turn,
        event: UsageEvent {
            harness: "amp".to_owned(),
            timestamp,
            session_id: session_id.to_owned(),
            message_id: resolved_message_id,
            turn,
            subagent: false,
            model,
            tokens: TokenCounts {
                input: input.max(0.0) as u64,
                output: output.max(0.0) as u64,
                cache_read: usage
                    .get("cacheReadInputTokens")
                    .and_then(finite_number)
                    .unwrap_or(0.0)
                    .max(0.0) as u64,
                cache_write: usage
                    .get("cacheCreationInputTokens")
                    .and_then(finite_number)
                    .unwrap_or(0.0)
                    .max(0.0) as u64,
                cache_write1h: None,
                reasoning: 0,
            },
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        },
    })
}

/// finiteNumber(x)?.toString() — JS number-to-string for an integer.
fn num_to_string(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Read all amp usage events from the given root. Faithful port of `readAmp`.
pub fn read_amp(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for file in list_files(root, |name| {
        name.starts_with("T-") && name.ends_with(".json")
    }) {
        let raw = match read_json(&file) {
            Some(r) => r,
            None => {
                skipped.push(file.clone());
                warnings.push(ReaderWarning {
                    message: format!("skipping unreadable amp thread {file}\n"),
                });
                continue;
            }
        };
        let message_store = if is_record(&raw) {
            raw.get("messages").and_then(Value::as_array)
        } else {
            None
        };
        let ledger_store = if is_record(&raw) {
            raw.get("usageLedger")
                .filter(|l| is_record(l))
                .and_then(|l| l.get("events"))
                .and_then(Value::as_array)
        } else {
            None
        };
        if !is_record(&raw) || (message_store.is_none() && ledger_store.is_none()) {
            skipped.push(file.clone());
            warnings.push(ReaderWarning {
                message: format!("skipping malformed amp thread {file}\n"),
            });
            continue;
        }
        let session_id = raw
            .get("id")
            .and_then(as_string)
            .unwrap_or_else(|| file.clone());
        let created = raw.get("created").and_then(finite_number);
        let fallback_timestamp = file_mtime_iso(&file);
        let empty: Vec<Value> = Vec::new();
        let messages = message_store.unwrap_or(&empty);
        let ledger = ledger_store.unwrap_or(&empty);

        let mut ledger_records: Vec<AmpRecord> = Vec::new();
        for (index, entry) in ledger.iter().enumerate() {
            if !is_record(entry) || !entry.get("tokens").map(is_record).unwrap_or(false) {
                continue;
            }
            let ledger_msg_id = entry
                .get("toMessageId")
                .and_then(finite_number)
                .map(num_to_string)
                .or_else(|| entry.get("id").and_then(as_string))
                .or(Some(format!("{file}:ledger:{index}")));
            let record = amp_record(
                &entry["tokens"],
                &session_id,
                ledger_msg_id,
                false,
                &fallback_timestamp,
                created,
                None,
                entry.get("model").and_then(as_string),
            );
            if let Some(mut record) = record {
                if let Some(ts) = entry.get("timestamp").and_then(as_string) {
                    record.event.timestamp = ts;
                }
                ledger_records.push(record);
            }
        }

        let mut consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut previous_was_user = false;
        for (index, message) in messages.iter().enumerate() {
            if !is_record(message) {
                continue;
            }
            let role = message.get("role").and_then(as_string);
            if role.as_deref() == Some("user") {
                previous_was_user = true;
                continue;
            }
            if role.as_deref() != Some("assistant") {
                continue;
            }
            let usage = message.get("usage");
            let usage = match usage {
                Some(u) if is_record(u) => u,
                _ => continue,
            };
            let message_id = message.get("messageId").and_then(as_string).or_else(|| {
                message
                    .get("messageId")
                    .and_then(finite_number)
                    .map(num_to_string)
            });
            let numeric_message_id = message.get("messageId").and_then(finite_number);
            let message_record = amp_record(
                usage,
                &session_id,
                message_id.clone(),
                previous_was_user,
                &message
                    .get("timestamp")
                    .and_then(as_string)
                    .unwrap_or_else(|| fallback_timestamp.clone()),
                created,
                numeric_message_id,
                message.get("model").and_then(as_string),
            );
            let mut message_record = match message_record {
                Some(r) => r,
                None => {
                    let location = format!("{file}:{}", index + 1);
                    skipped.push(location.clone());
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed amp message {location}\n"),
                    });
                    previous_was_user = false;
                    continue;
                }
            };
            let message_timestamp = message.get("timestamp").and_then(as_string);
            if let Some(ts) = &message_timestamp
                && usage.get("timestamp").and_then(as_string).is_none()
            {
                message_record.event.timestamp = ts.clone();
            }

            let ledger_index = ledger_records
                .iter()
                .enumerate()
                .position(|(i, record)| !consumed.contains(&i) && record.message_id == message_id);
            let matching_index = match ledger_index {
                Some(i) => Some(i),
                None => ledger_records.iter().enumerate().position(|(i, record)| {
                    !consumed.contains(&i)
                        && record.event.model == message_record.event.model
                        && record.event.tokens.input == message_record.event.tokens.input
                        && record.event.tokens.output == message_record.event.tokens.output
                }),
            };

            let resolved_event: UsageEvent = match matching_index {
                Some(mi) => {
                    consumed.insert(mi);
                    let mr_turn = message_record.turn;
                    let record = &mut ledger_records[mi];
                    record.turn = mr_turn;
                    record.event.turn = mr_turn;
                    record.event.message_id = message_id
                        .clone()
                        .unwrap_or_else(|| record.event.message_id.clone());
                    record.event.tokens.cache_read = message_record.event.tokens.cache_read;
                    record.event.tokens.cache_write = message_record.event.tokens.cache_write;
                    record.event.clone()
                }
                None => message_record.event.clone(),
            };

            let key = format!("{session_id}:{}", resolved_event.message_id);
            if !seen.contains(&key) {
                seen.insert(key);
                events.push(resolved_event);
            }
            previous_was_user = false;
        }

        for (index, record) in ledger_records.iter().enumerate() {
            if consumed.contains(&index) {
                continue;
            }
            let key = format!("{session_id}:{}", record.event.message_id);
            if seen.insert(key) {
                events.push(record.event.clone());
            }
        }
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

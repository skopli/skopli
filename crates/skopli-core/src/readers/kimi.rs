use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    EpochUnit, ReaderResult, ReaderWarning, as_string, basename, date_parse_ms, dirname,
    epoch_to_iso_unit, file_mtime_iso, finite_number, is_record, list_files, map_chunks, read_json,
    read_jsonl_lines,
};
use crate::types::{TokenCounts, UsageEvent};

/// The kimi reader wired into the harness registry.
pub struct KimiReader;

impl Reader for KimiReader {
    fn harness_id(&self) -> &'static str {
        "kimi"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = kimi_roots(ctx.env("KIMI_DATA_DIR"), ctx.home());
        read_kimi(&roots)
    }
}

/// Resolve kimi roots. `KIMI_DATA_DIR` (comma-split, trimmed, non-empty) wins;
/// else `~/.kimi` + `~/.kimi-code`.
pub fn kimi_roots(override_dir: Option<&str>, home: &Path) -> Vec<PathBuf> {
    if let Some(override_val) = override_dir
        && !override_val.is_empty()
    {
        return override_val
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    vec![home.join(".kimi"), home.join(".kimi-code")]
}

const DEFAULT_MODEL: &str = "kimi-for-coding";

/// `config.json`'s non-empty `model`, else the default.
fn config_model(root: &Path) -> String {
    let config_path = root.join("config.json");
    if let Some(config) = read_json(&config_path.to_string_lossy())
        && is_record(&config)
        && let Some(model) = config.get("model").and_then(as_string)
        && !model.trim().is_empty()
    {
        return model;
    }
    DEFAULT_MODEL.to_owned()
}

fn session_id_from_path(file: &str) -> String {
    let parent = dirname(file);
    let session_dir = if basename(&dirname(&parent)) == "agents" {
        dirname(&dirname(&parent))
    } else {
        parent
    };
    basename(&session_dir)
}

/// Strip the `kimi-code/` prefix and reject empty or symbolic (`__x__`)
/// references.
fn concrete_kimi_model(raw: Option<String>) -> Option<String> {
    let raw = raw?;
    let mut model = raw.trim().to_owned();
    if let Some(stripped) = model.strip_prefix("kimi-code/") {
        model = stripped.trim().to_owned();
    }
    if model.is_empty() {
        return None;
    }
    let symbolic = model.len() >= 4 && model.starts_with("__") && model.ends_with("__");
    if symbolic { None } else { Some(model) }
}

fn token_total(tokens: &TokenCounts) -> u64 {
    tokens.input + tokens.output + tokens.cache_read + tokens.cache_write + tokens.reasoning
}

fn usage_tokens(value: Option<&Value>) -> Option<TokenCounts> {
    let value = value?;
    if !is_record(value) {
        return None;
    }
    let pick = |keys: &[&str]| -> u64 {
        for key in keys {
            if let Some(found) = value.get(*key).and_then(finite_number) {
                return found.max(0.0) as u64;
            }
        }
        0
    };
    let mut tokens = TokenCounts {
        input: pick(&["input_other", "inputOther"]),
        output: pick(&["output"]),
        cache_read: pick(&["input_cache_read", "inputCacheRead"]),
        cache_write: pick(&["input_cache_creation", "inputCacheCreation"]),
        cache_write1h: None,
        reasoning: 0,
    };
    let total = pick(&["total"]);
    let known = tokens.input + tokens.output + tokens.cache_read + tokens.cache_write;
    if total > known {
        if tokens.output == 0 {
            tokens.output = total - known;
        } else {
            tokens.reasoning = total - known;
        }
    }
    if tokens.input == 0 && tokens.output == 0 && tokens.cache_read == 0 && tokens.cache_write == 0
    {
        return None;
    }
    Some(tokens)
}

/// Where a StatusUpdate's timestamp came from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TimestampSource {
    Wire,
    Mtime,
}

struct StatusSlot {
    event: UsageEvent,
    total: u64,
    source: TimestampSource,
}

fn should_replace(existing: &StatusSlot, candidate: &StatusSlot) -> bool {
    if candidate.total != existing.total {
        return candidate.total > existing.total;
    }
    if candidate.source != existing.source {
        return candidate.source == TimestampSource::Wire;
    }
    let cand_ms = date_parse_ms(&candidate.event.timestamp);
    let exist_ms = date_parse_ms(&existing.event.timestamp);
    match (cand_ms, exist_ms) {
        (Some(c), Some(e)) => c >= e,
        // An unparseable timestamp on either side never replaces.
        _ => false,
    }
}

struct KeyedSlot {
    key: String,
    slot: StatusSlot,
}

struct WireResult {
    events: Vec<UsageEvent>,
    slots: Vec<KeyedSlot>,
}

fn read_wire_file(
    file: &str,
    config_fallback_model: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> WireResult {
    let lines = match read_jsonl_lines(file, skipped, warnings) {
        Some(l) => l,
        None => {
            return WireResult {
                events: Vec::new(),
                slots: Vec::new(),
            };
        }
    };
    let session_id = session_id_from_path(file);
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut slots: Vec<KeyedSlot> = Vec::new();
    let mut latest_request_model: Option<String> = None;

    for line in &lines {
        if !is_record(&line.value) {
            continue;
        }
        let record = &line.value;
        let type_ = record.get("type").and_then(Value::as_str);
        if type_ == Some("metadata") {
            continue;
        }

        if type_ == Some("llm.request") {
            let model = concrete_kimi_model(record.get("model").and_then(as_string));
            if model.is_some() {
                latest_request_model = model;
            }
            continue;
        }

        if type_ == Some("usage.record") {
            // only turn-scope records; a missing scope is session-scoped
            if record.get("usageScope").and_then(Value::as_str) != Some("turn") {
                continue;
            }
            let tokens = match usage_tokens(record.get("usage")) {
                Some(t) => t,
                None => continue,
            };
            let timestamp =
                epoch_to_iso_unit(record.get("time").unwrap_or(&Value::Null), EpochUnit::Ms)
                    .unwrap_or_else(|| file_mtime_iso(file));
            let model = concrete_kimi_model(record.get("model").and_then(as_string))
                .or_else(|| latest_request_model.clone())
                .unwrap_or_else(|| config_fallback_model.to_owned());
            events.push(UsageEvent {
                harness: "kimi".to_owned(),
                timestamp,
                session_id: session_id.clone(),
                message_id: format!("{file}:{}", line.index),
                turn: true,
                subagent: false,
                model,
                tokens,
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            });
            continue;
        }

        let message = match record.get("message") {
            Some(m) if is_record(m) => m,
            _ => continue,
        };
        if message.get("type").and_then(Value::as_str) != Some("StatusUpdate") {
            continue;
        }
        let payload = match message.get("payload") {
            Some(p) if is_record(p) => p,
            _ => continue,
        };
        let tokens = match usage_tokens(payload.get("token_usage")) {
            Some(t) => t,
            None => continue,
        };
        let wire_timestamp = epoch_to_iso_unit(
            record.get("timestamp").unwrap_or(&Value::Null),
            EpochUnit::Sec,
        );
        let source = if wire_timestamp.is_some() {
            TimestampSource::Wire
        } else {
            TimestampSource::Mtime
        };
        let timestamp = wire_timestamp.unwrap_or_else(|| file_mtime_iso(file));
        let message_id = payload
            .get("message_id")
            .and_then(as_string)
            .unwrap_or_default();
        let total = token_total(&tokens);
        let event = UsageEvent {
            harness: "kimi".to_owned(),
            timestamp,
            session_id: session_id.clone(),
            message_id: if message_id.is_empty() {
                format!("{file}:{}", line.index)
            } else {
                message_id.clone()
            },
            turn: true,
            subagent: false,
            model: config_fallback_model.to_owned(),
            tokens,
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        };
        if message_id.is_empty() {
            events.push(event);
            continue;
        }
        slots.push(KeyedSlot {
            key: format!("{session_id}:{message_id}"),
            slot: StatusSlot {
                event,
                total,
                source,
            },
        });
    }
    WireResult { events, slots }
}

fn read_wire_files(
    files: &[String],
    config_fallback_model: &str,
) -> (Vec<WireResult>, Vec<String>, Vec<ReaderWarning>) {
    let (mut skipped, mut warnings) = (Vec::new(), Vec::new());
    let results = files
        .iter()
        .map(|file| read_wire_file(file, config_fallback_model, &mut skipped, &mut warnings))
        .collect();
    (results, skipped, warnings)
}

/// Read all kimi usage events.
pub fn read_kimi(roots: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // insertion order is the emission order, so keep it beside the map
    let mut keyed_order: Vec<String> = Vec::new();
    let mut keyed: HashMap<String, StatusSlot> = HashMap::new();

    let mut add_event = |event: UsageEvent, events: &mut Vec<UsageEvent>| {
        let tokens_json = serde_json::to_string(&event.tokens).unwrap_or_default();
        let key = format!(
            "{}:{}:{}:{}",
            event.session_id, event.message_id, event.model, tokens_json
        );
        if seen.contains(&key) {
            return;
        }
        seen.insert(key);
        events.push(event);
    };

    for root in roots {
        let sessions_dir = root.join("sessions");
        let files = list_files(&sessions_dir, |name| name == "wire.jsonl");
        let model = config_model(root);
        for (results, part_skipped, part_warnings) in
            map_chunks(&files, |chunk| read_wire_files(chunk, &model))
        {
            skipped.extend(part_skipped);
            warnings.extend(part_warnings);
            for WireResult {
                events: direct,
                slots,
            } in results
            {
                for event in direct {
                    add_event(event, &mut events);
                }
                for KeyedSlot { key, slot } in slots {
                    match keyed.get(&key) {
                        Some(existing) if !should_replace(existing, &slot) => {}
                        Some(_) => {
                            keyed.insert(key, slot);
                        }
                        None => {
                            keyed_order.push(key.clone());
                            keyed.insert(key, slot);
                        }
                    }
                }
            }
        }
    }
    for key in &keyed_order {
        if let Some(slot) = keyed.remove(key) {
            add_event(slot.event, &mut events);
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
    use std::fs;

    use super::KimiReader;
    use crate::readers::reader::{Reader, ReaderContext};
    use crate::readers::shared::{list_files, scratch_dir};

    fn status_line(output: u64) -> String {
        format!(
            "{{\"timestamp\":1700000000,\"message\":{{\"type\":\"StatusUpdate\",\"payload\":{{\"message_id\":\"m\",\"token_usage\":{{\"input_other\":1,\"output\":{output}}}}}}}}}"
        )
    }

    #[test]
    fn many_files_keep_session_order_and_replace_slots_across_chunks() {
        let home = scratch_dir("kimi-many");
        let sessions = home.join(".kimi").join("sessions");
        // s00's forty agent files sort before its main file, so whenever there
        // is more than one thread the main file is in a later chunk, and its
        // larger total must replace the agents' slot.
        for i in 0..40 {
            let session = sessions.join(format!("s{i:02}"));
            let agent = sessions.join(format!("s00/agents/a{i:02}"));
            fs::create_dir_all(&session).unwrap();
            fs::create_dir_all(&agent).unwrap();
            let agent_line = format!("{}\nnot json", status_line(1));
            fs::write(agent.join("wire.jsonl"), agent_line).unwrap();
            let main = format!("{}\nnot json", status_line(5));
            fs::write(session.join("wire.jsonl"), main).unwrap();
        }
        let result = KimiReader.read(&ReaderContext::new(&*home));
        let got: Vec<(String, u64)> = result
            .events
            .iter()
            .map(|e| (e.session_id.clone(), e.tokens.output))
            .collect();
        let expected: Vec<(String, u64)> = (0..40).map(|i| (format!("s{i:02}"), 5)).collect();
        assert_eq!(got, expected);
        let files = list_files(&sessions, |name| name == "wire.jsonl");
        let skipped: Vec<String> = files.iter().map(|f| format!("{f}:2")).collect();
        assert_eq!(result.skipped, skipped);
    }
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, file_mtime_iso, file_stem_name, finite_number,
    is_record, list_files, read_json, reparse_iso,
};
use crate::types::{TokenCounts, UsageEvent};

/// The augment reader wired into the harness registry. Resolves its sessions
/// root from `AUGMENT_SESSIONS_DIR` (else `~/.augment/sessions`).
pub struct AugmentReader;

impl Reader for AugmentReader {
    fn harness_id(&self) -> &'static str {
        "augment"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = augment_sessions_root(ctx.env("AUGMENT_SESSIONS_DIR"), ctx.home());
        read_augment(&root)
    }
}

/// Resolve the augment sessions root. Faithful port of `augmentSessionsRoot`.
pub fn augment_sessions_root(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join(".augment").join("sessions")
}

/// Faithful port of `usageTokens`: each bucket is `max(0, finite ?? 0)`; input
/// and cache buckets are independent (never subtracted). Returns `None` when all
/// four token buckets are zero.
fn usage_tokens(value: &Value) -> Option<TokenCounts> {
    if !is_record(value) {
        return None;
    }
    let pick = |key: &str| -> u64 {
        value
            .get(key)
            .and_then(finite_number)
            .unwrap_or(0.0)
            .max(0.0) as u64
    };
    let tokens = TokenCounts {
        input: pick("input_tokens"),
        output: pick("output_tokens"),
        cache_read: pick("cache_read_input_tokens"),
        cache_write: pick("cache_creation_input_tokens"),
        cache_write1h: None,
        reasoning: 0,
    };
    if tokens.input == 0 && tokens.output == 0 && tokens.cache_read == 0 && tokens.cache_write == 0
    {
        return None;
    }
    Some(tokens)
}

/// Faithful port of `lastTokenUsage`: a turn may stream several usage-bearing
/// nodes; the last non-empty one is the full total, so summing would
/// double-count. Scans from the end, returns the first non-empty usage found.
fn last_token_usage(nodes: Option<&Value>) -> Option<TokenCounts> {
    let arr = match nodes {
        Some(Value::Array(a)) => a,
        _ => return None,
    };
    for node in arr.iter().rev() {
        if !is_record(node) {
            continue;
        }
        if let Some(tokens) = node.get("token_usage").and_then(usage_tokens) {
            return Some(tokens);
        }
    }
    None
}

/// Faithful port of `turnKey`.
fn turn_key(session_id: &str, turn: &Value, index: usize) -> String {
    let exchange = turn.get("exchange").filter(|e| is_record(e));
    let request_id = exchange
        .and_then(|e| e.get("request_id"))
        .and_then(as_string);
    if let Some(req) = request_id
        && !req.trim().is_empty()
    {
        return format!("augment:{session_id}:req:{}", req.trim());
    }
    // sequenceId may be any JSON scalar; TS stringifies non-strings via
    // JSON.stringify and rejects "" and "null".
    if let Some(sequence) = turn.get("sequenceId")
        && !sequence.is_null()
    {
        let text = match sequence {
            Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };
        if !text.is_empty() && text != "null" {
            return format!("augment:{session_id}:seq:{text}");
        }
    }
    format!("augment:{session_id}:turn:{index}")
}

/// Read all augment usage events. Faithful port of `readAugment`.
pub fn read_augment(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for file in list_files(root, |name| name.ends_with(".json")) {
        let session = match read_json(&file) {
            Some(v) => v,
            None => {
                warnings.push(ReaderWarning {
                    message: format!("skipping unreadable file {file}\n"),
                });
                skipped.push(file.clone());
                continue;
            }
        };
        if !is_record(&session) {
            continue;
        }
        let from_file = file_stem_name(&file);
        let id_field = session.get("sessionId").and_then(as_string);
        let session_id = match id_field {
            Some(id) if !id.trim().is_empty() => id.trim().to_owned(),
            _ => from_file,
        };
        if session_id.is_empty() {
            continue;
        }
        let agent_state = session.get("agentState").filter(|a| is_record(a));
        let default_model = agent_state
            .and_then(|a| a.get("modelId"))
            .and_then(as_string)
            .unwrap_or_default();
        let history = match session.get("chatHistory") {
            Some(Value::Array(h)) => h,
            _ => continue,
        };
        let fallback_timestamp = file_mtime_iso(&file);
        for (index, raw_turn) in history.iter().enumerate() {
            if !is_record(raw_turn) {
                continue;
            }
            // snapshots retain in-progress or aborted turns with partial usage
            if raw_turn.get("completed") != Some(&Value::Bool(true)) {
                continue;
            }
            let exchange = match raw_turn.get("exchange") {
                Some(e) if is_record(e) => e,
                _ => continue,
            };
            let tokens = match last_token_usage(exchange.get("response_nodes")) {
                Some(t) => t,
                None => continue,
            };
            let exchange_model = exchange.get("model_id").and_then(as_string);
            let model = match exchange_model {
                Some(m) if !m.trim().is_empty() => m.trim().to_owned(),
                _ => {
                    if !default_model.trim().is_empty() {
                        default_model.trim().to_owned()
                    } else {
                        "unknown".to_owned()
                    }
                }
            };
            let finished_at = raw_turn.get("finishedAt").and_then(as_string);
            let timestamp = finished_at
                .as_deref()
                .and_then(reparse_iso)
                .unwrap_or_else(|| fallback_timestamp.clone());
            let key = turn_key(&session_id, raw_turn, index);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key.clone());
            events.push(UsageEvent {
                harness: "augment".to_owned(),
                timestamp,
                session_id: session_id.clone(),
                message_id: key,
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
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

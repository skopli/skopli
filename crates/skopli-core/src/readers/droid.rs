use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, date_parse_ms, file_mtime_iso, file_stem_name,
    finite_number, is_record, list_files, read_json, reparse_iso,
};
use crate::types::{TokenCounts, UsageEvent};

/// The droid reader wired into the harness registry.
pub struct DroidReader;

impl Reader for DroidReader {
    fn harness_id(&self) -> &'static str {
        "droid"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = droid_sessions_root(ctx.env("DROID_SESSIONS_DIR"), ctx.home());
        read_droid(&root)
    }
}

/// Resolve the droid sessions root. Faithful port of `droidSessionsRoot`.
pub fn droid_sessions_root(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join(".factory").join("sessions")
}

/// Faithful port of `normalizeModel`.
fn normalize_model(model: &str) -> String {
    // strip a leading "custom:" then remove any "[...]" bracketed segments
    let without_custom = strip_prefix_custom(model);
    let without_brackets = remove_brackets(&without_custom);
    let trimmed = without_brackets.trim();
    // replace runs of "." and whitespace with "-"
    let mut collapsed = String::with_capacity(trimmed.len());
    let mut prev_dash = false;
    for c in trimmed.chars() {
        if c == '.' || c.is_whitespace() {
            if !prev_dash {
                collapsed.push('-');
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }
    // collapse consecutive dashes, strip leading/trailing dash, lowercase
    let mut result = String::with_capacity(collapsed.len());
    let mut last_dash = false;
    for c in collapsed.chars() {
        if c == '-' {
            if !last_dash {
                result.push('-');
            }
            last_dash = true;
        } else {
            result.push(c);
            last_dash = false;
        }
    }
    result.trim_matches('-').to_lowercase()
}

fn strip_prefix_custom(model: &str) -> String {
    model.strip_prefix("custom:").unwrap_or(model).to_owned()
}

fn remove_brackets(model: &str) -> String {
    let mut out = String::with_capacity(model.len());
    let mut depth = 0usize;
    for c in model.chars() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Faithful port of `sidecarModel`: reads the sibling `.jsonl`, matches
/// `Model:\s*([^"\\[]+)` and normalizes the capture.
fn sidecar_model(file: &str) -> Option<String> {
    let jsonl = if let Some(stripped) = file.strip_suffix(".settings.json") {
        format!("{stripped}.jsonl")
    } else {
        return None;
    };
    let content = fs::read_to_string(&jsonl).ok()?;
    let idx = content.find("Model:")?;
    let after = &content[idx + "Model:".len()..];
    let after = after.trim_start_matches([' ', '\t']);
    // capture group [^"\\[]+ : up to (but excluding) '"', '\\', or '['
    let end = after.find(['"', '\\', '[']).unwrap_or(after.len());
    let captured = &after[..end];
    Some(normalize_model(captured))
}

/// Faithful port of `providerModel`.
fn provider_model(provider: Option<&str>) -> String {
    match provider.map(str::to_lowercase).as_deref() {
        Some("anthropic") | Some("claude") => "claude-unknown".to_owned(),
        Some("openai") => "gpt-unknown".to_owned(),
        Some("google") | Some("gemini") => "gemini-unknown".to_owned(),
        _ => "unknown".to_owned(),
    }
}

struct Entry {
    event: UsageEvent,
    modified: i64,
}

/// Read all droid usage events. Faithful port of `readDroid`.
pub fn read_droid(root: &Path) -> ReaderResult {
    let mut by_session: HashMap<String, Entry> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    for file in list_files(root, |name| name.ends_with(".settings.json")) {
        let raw = match read_json(&file) {
            Some(v) => v,
            None => {
                skipped.push(file.clone());
                warnings.push(ReaderWarning {
                    message: format!("skipping unreadable droid session {file}\n"),
                });
                continue;
            }
        };
        let usage = if is_record(&raw) {
            raw.get("tokenUsage")
        } else {
            None
        };
        let usage = match usage {
            Some(u) if is_record(u) => u,
            _ => {
                skipped.push(file.clone());
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed droid session {file}\n"),
                });
                continue;
            }
        };
        let input = usage.get("inputTokens").and_then(finite_number);
        let output = usage.get("outputTokens").and_then(finite_number);
        let (input, output) = match (input, output) {
            (Some(i), Some(o)) => (i, o),
            _ => {
                skipped.push(file.clone());
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed droid session {file}\n"),
                });
                continue;
            }
        };
        let session_id = raw
            .get("session_id")
            .and_then(as_string)
            .or_else(|| raw.get("id").and_then(as_string))
            .unwrap_or_else(|| {
                let name = file_stem_name(&file);
                name.strip_suffix(".settings").unwrap_or(&name).to_owned()
            });
        let timestamp_value = raw.get("providerLockTimestamp").and_then(as_string);
        let timestamp = timestamp_value
            .as_deref()
            .and_then(reparse_iso)
            .unwrap_or_else(|| file_mtime_iso(&file));
        let modified = date_parse_ms(&file_mtime_iso(&file)).unwrap_or(0);
        if let Some(prior) = by_session.get(&session_id)
            && prior.modified > modified
        {
            continue;
        }
        let model = raw.get("model").and_then(as_string);
        let resolved_model = match model {
            Some(m) => normalize_model(&m),
            None => sidecar_model(&file)
                .unwrap_or_else(|| provider_model(raw.get("providerLock").and_then(Value::as_str))),
        };
        let event = UsageEvent {
            harness: "droid".to_owned(),
            timestamp,
            session_id: session_id.clone(),
            message_id: session_id.clone(),
            turn: false,
            subagent: false,
            model: resolved_model,
            tokens: TokenCounts {
                input: input.max(0.0) as u64,
                output: output.max(0.0) as u64,
                cache_read: finite_number_of(usage, "cacheReadTokens"),
                cache_write: finite_number_of(usage, "cacheCreationTokens"),
                cache_write1h: None,
                reasoning: finite_number_of(usage, "thinkingTokens"),
            },
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        };
        if by_session
            .insert(session_id.clone(), Entry { event, modified })
            .is_none()
        {
            order.push(session_id);
        }
    }

    // sort by modified ascending (stable within equal keys via insertion order)
    let mut entries: Vec<Entry> = order
        .into_iter()
        .filter_map(|k| by_session.remove(&k))
        .collect();
    entries.sort_by(|a, b| a.modified.cmp(&b.modified));
    let events = entries.into_iter().map(|e| e.event).collect();
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

fn finite_number_of(usage: &Value, key: &str) -> u64 {
    usage
        .get(key)
        .and_then(finite_number)
        .unwrap_or(0.0)
        .max(0.0) as u64
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    EpochUnit, ReaderResult, ReaderWarning, epoch_to_iso_unit, file_mtime_iso, finite_number,
    is_record, list_files, parent_dir_name, read_json,
};
use crate::types::{TokenCounts, UsageEvent};

/// The mux reader wired into the harness registry.
pub struct MuxReader;

impl Reader for MuxReader {
    fn harness_id(&self) -> &'static str {
        "mux"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = mux_sessions_root(ctx.env("MUX_SESSIONS_DIR"), ctx.home());
        read_mux(&root)
    }
}

/// Resolve the mux sessions root. Faithful port of `muxSessionsRoot`.
pub fn mux_sessions_root(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join(".mux").join("sessions")
}

/// Faithful port of `bucketTokens`: `max(0, finite(value.tokens) ?? 0)`.
fn bucket_tokens(value: Option<&Value>) -> u64 {
    let value = match value {
        Some(v) if is_record(v) => v,
        _ => return 0,
    };
    value
        .get("tokens")
        .and_then(finite_number)
        .unwrap_or(0.0)
        .max(0.0) as u64
}

/// Read all mux usage events. Faithful port of `readMux`.
pub fn read_mux(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for file in list_files(root, |name| name == "session-usage.json") {
        let usage = match read_json(&file) {
            Some(v) => v,
            None => {
                warnings.push(ReaderWarning {
                    message: format!("skipping unreadable file {file}\n"),
                });
                skipped.push(file.clone());
                continue;
            }
        };
        if !is_record(&usage) {
            continue;
        }
        let session_id = parent_dir_name(&file);
        let by_model = match usage.get("byModel") {
            Some(v) if is_record(v) => v,
            _ => continue,
        };
        let last_request = usage.get("lastRequest").filter(|v| is_record(v));
        let timestamp = last_request
            .and_then(|lr| {
                epoch_to_iso_unit(lr.get("timestamp").unwrap_or(&Value::Null), EpochUnit::Ms)
            })
            .unwrap_or_else(|| file_mtime_iso(&file));
        for (model_key, model_usage) in by_model.as_object().unwrap() {
            if !is_record(model_usage) {
                continue;
            }
            let tokens = TokenCounts {
                input: bucket_tokens(model_usage.get("input")),
                output: bucket_tokens(model_usage.get("output")),
                cache_read: bucket_tokens(model_usage.get("cached")),
                cache_write: bucket_tokens(model_usage.get("cacheCreate")),
                cache_write1h: None,
                reasoning: bucket_tokens(model_usage.get("reasoning")),
            };
            if tokens.input == 0
                && tokens.output == 0
                && tokens.cache_read == 0
                && tokens.cache_write == 0
                && tokens.reasoning == 0
            {
                continue;
            }
            let model = match model_key.find(':') {
                Some(colon) => model_key[colon + 1..].to_owned(),
                None => model_key.clone(),
            };
            // stable per-workspace key so a re-read collapses the same entry
            let key = format!("mux:{session_id}:{model_key}");
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key.clone());
            events.push(UsageEvent {
                harness: "mux".to_owned(),
                timestamp: timestamp.clone(),
                session_id: session_id.clone(),
                message_id: key,
                // per-session/model cumulative bucket, not a real turn boundary
                turn: false,
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

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, ScanJsonl, as_string, basename, epoch_to_iso,
    file_mtime_iso, finite_number, is_record, list_files, reparse_iso, scan_jsonl,
};
use crate::types::{TokenCounts, UsageEvent};

/// The openclaw reader wired into the harness registry. Faithful port of
/// `readOpenclaw` in src/readers/openclaw.ts.
pub struct OpenclawReader;

impl Reader for OpenclawReader {
    fn harness_id(&self) -> &'static str {
        "openclaw"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = openclaw_roots(ctx.env("OPENCLAW_DIR"), ctx.home());
        read_openclaw(&roots)
    }
}

/// Resolve openclaw roots. Faithful port of `openclawRoots`: `OPENCLAW_DIR`
/// (comma-split, trimmed, non-empty) wins; else the four default `~/.*bot` dirs.
pub fn openclaw_roots(override_dir: Option<&str>, home: &Path) -> Vec<PathBuf> {
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
    vec![
        home.join(".openclaw"),
        home.join(".clawdbot"),
        home.join(".moltbot"),
        home.join(".moldbot"),
    ]
}

/// Faithful port of `isModelChange`.
fn is_model_change(record: &Value) -> bool {
    if record.get("type").and_then(Value::as_str) == Some("model_change") {
        return true;
    }
    record.get("type").and_then(Value::as_str) == Some("custom")
        && record.get("customType").and_then(Value::as_str) == Some("model-snapshot")
}

/// Faithful port of `isTranscriptFile`: `.jsonl`, or a rotated `.jsonl.deleted.`
/// / `.jsonl.reset.` archive.
fn is_transcript_file(name: &str) -> bool {
    name.ends_with(".jsonl") || name.contains(".jsonl.deleted.") || name.contains(".jsonl.reset.")
}

/// Faithful port of `sessionIdFromName`: the filename up to the first `.jsonl`,
/// or `None` when nothing precedes it.
fn session_id_from_name(name: &str) -> Option<String> {
    let index = name.find(".jsonl")?;
    if index == 0 {
        return None;
    }
    Some(name[..index].to_owned())
}

/// Faithful port of `usageTokens` in openclaw.ts.
fn usage_tokens(value: Option<&Value>) -> Option<TokenCounts> {
    let value = value?;
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
    let mut tokens = TokenCounts {
        input: pick("input"),
        output: pick("output"),
        cache_read: pick("cacheRead"),
        cache_write: pick("cacheWrite"),
        cache_write1h: None,
        reasoning: 0,
    };
    let total = pick("totalTokens");
    let known = tokens.input + tokens.output + tokens.cache_read + tokens.cache_write;
    if total > known && tokens.output == 0 {
        tokens.output = total - known;
    }
    if tokens.input == 0 && tokens.output == 0 && tokens.cache_read == 0 && tokens.cache_write == 0
    {
        return None;
    }
    Some(tokens)
}

/// Faithful port of `timestampFrom` in openclaw.ts: an epoch (auto) value, else
/// an ISO/parseable string, else the fallback.
fn timestamp_from(value: Option<&Value>, fallback: &str) -> String {
    let value = value.unwrap_or(&Value::Null);
    if let Some(from_epoch) = epoch_to_iso(value) {
        return from_epoch;
    }
    if let Some(text) = as_string(value)
        && let Some(iso) = reparse_iso(&text)
    {
        return iso;
    }
    fallback.to_owned()
}

struct OpenclawEntry {
    event: UsageEvent,
    key: String,
}

/// Read all openclaw usage events. Faithful port of `readOpenclaw`.
pub fn read_openclaw(roots: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for root in roots {
        let files = list_files(root, is_transcript_file);
        for file in &files {
            let fallback_timestamp = file_mtime_iso(file);
            let session_id = match session_id_from_name(&basename(file)) {
                Some(s) => s,
                None => continue,
            };
            // model/provider state is carried across lines by model-change records
            let mut current_model: Option<String> = None;
            let mut current_provider: Option<String> = None;

            let files_slice = std::slice::from_ref(file);
            let mut parse = |line: &JsonlLine, file: &str| -> Option<OpenclawEntry> {
                if !is_record(&line.value) {
                    return None;
                }
                let record = &line.value;
                if is_model_change(record) {
                    let data = record.get("data").filter(|d| is_record(d));
                    let model = data
                        .and_then(|d| {
                            d.get("modelId")
                                .and_then(as_string)
                                .or_else(|| d.get("model").and_then(as_string))
                        })
                        .or_else(|| record.get("modelId").and_then(as_string))
                        .or_else(|| record.get("model").and_then(as_string));
                    let provider = data
                        .and_then(|d| d.get("provider").and_then(as_string))
                        .or_else(|| record.get("provider").and_then(as_string));
                    if let Some(m) = model
                        && !m.is_empty()
                    {
                        current_model = Some(m);
                    }
                    if let Some(p) = provider
                        && !p.is_empty()
                    {
                        current_provider = Some(p);
                    }
                    return None;
                }
                if record.get("type").and_then(Value::as_str) != Some("message") {
                    return None;
                }
                let message = record.get("message");
                // a malformed message must not drop the line's model state
                let message = match message {
                    Some(m) if is_record(m) => m,
                    _ => return None,
                };
                if message.get("role").and_then(Value::as_str) != Some("assistant") {
                    return None;
                }
                let tokens = usage_tokens(message.get("usage"))?;
                // a model named inline on a message applies to that message only;
                // it deliberately does not update currentModel
                let model = message
                    .get("modelId")
                    .and_then(as_string)
                    .or_else(|| message.get("model").and_then(as_string))
                    .or_else(|| current_model.clone())
                    .unwrap_or_else(|| "unknown".to_owned());
                let provider = message
                    .get("provider")
                    .and_then(as_string)
                    .or_else(|| current_provider.clone());
                let timestamp = timestamp_from(
                    message.get("timestamp").or_else(|| record.get("timestamp")),
                    &fallback_timestamp,
                );
                let resolved_model = match provider.as_deref() {
                    None | Some("") => model,
                    Some(p) => format!("{p}/{model}"),
                };
                let tokens_json = serde_json::to_string(&tokens).unwrap_or_default();
                let key = format!("{session_id}:{timestamp}:{resolved_model}:{tokens_json}");
                Some(OpenclawEntry {
                    event: UsageEvent {
                        harness: "openclaw".to_owned(),
                        timestamp,
                        session_id: session_id.clone(),
                        message_id: format!("{file}:{}", line.index),
                        turn: true,
                        subagent: false,
                        model: resolved_model,
                        tokens,
                        calls: None,
                        cost_usd: None,
                        workspace: None,
                        title: None,
                    },
                    key,
                })
            };

            let entries = scan_jsonl(
                ScanJsonl {
                    files: files_slice,
                    parse: &mut parse,
                    dedup_key: None,
                },
                &mut skipped,
                &mut warnings,
            );
            for entry in entries {
                if seen.contains(&entry.key) {
                    continue;
                }
                seen.insert(entry.key);
                events.push(entry.event);
            }
        }
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

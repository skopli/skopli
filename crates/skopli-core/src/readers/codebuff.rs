use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, basename, epoch_to_iso, file_mtime_iso, finite_number,
    is_record, list_files, read_json, reparse_iso,
};
use crate::types::{TokenCounts, UsageEvent};

/// The codebuff reader wired into the harness registry.
pub struct CodebuffReader;

impl Reader for CodebuffReader {
    fn harness_id(&self) -> &'static str {
        "codebuff"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = codebuff_roots(ctx.env("CODEBUFF_DATA_DIR"), ctx.home());
        read_codebuff(&roots)
    }
}

/// Resolve codebuff project roots. Faithful port of `codebuffRoots`: the
/// `CODEBUFF_DATA_DIR` override (comma-split, trimmed, non-empty) wins, else the
/// three default `~/.config/manicode*` bases; each base gets `/projects`
/// appended unless it already ends in `projects`.
pub fn codebuff_roots(config_dir: Option<&str>, home: &Path) -> Vec<PathBuf> {
    let bases: Vec<PathBuf> = match config_dir {
        Some(v) if !v.is_empty() => v
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ => vec![
            home.join(".config").join("manicode"),
            home.join(".config").join("manicode-dev"),
            home.join(".config").join("manicode-staging"),
        ],
    };
    bases
        .into_iter()
        .map(|base| {
            if basename(&base.to_string_lossy()) == "projects" {
                base
            } else {
                base.join("projects")
            }
        })
        .collect()
}

const DEFAULT_MODEL: &str = "codebuff-unknown";

/// A partially-resolved usage accumulator. Mirrors the TS `Usage` type.
#[derive(Clone)]
struct Usage {
    model: Option<String>,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

fn empty_usage() -> Usage {
    Usage {
        model: None,
        input: 0.0,
        output: 0.0,
        cache_read: 0.0,
        cache_write: 0.0,
    }
}

fn has_signal(usage: &Usage) -> bool {
    usage.input > 0.0 || usage.output > 0.0 || usage.cache_read > 0.0 || usage.cache_write > 0.0
}

/// Later sources only fill buckets the earlier, more authoritative source left
/// empty; they never overwrite. Faithful port of `mergeFallback`.
fn merge_fallback(into: &mut Usage, from: &Usage) {
    if into.input <= 0.0 {
        into.input = from.input;
    }
    if into.output <= 0.0 {
        into.output = from.output;
    }
    if into.cache_read <= 0.0 {
        into.cache_read = from.cache_read;
    }
    if into.cache_write <= 0.0 {
        into.cache_write = from.cache_write;
    }
    if into.model.is_none() {
        into.model = from.model.clone();
    }
}

/// Faithful port of `pickNumber`: the first finite key wins, clamped to `>= 0`.
fn pick_number(value: &Value, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(found) = value.get(*key).and_then(finite_number) {
            return found.max(0.0);
        }
    }
    0.0
}

/// Faithful port of `parseUsageObject`.
fn parse_usage_object(value: Option<&Value>) -> Usage {
    let mut usage = empty_usage();
    let value = match value {
        Some(v) if is_record(v) => v,
        _ => return usage,
    };
    usage.input = pick_number(
        value,
        &[
            "inputTokens",
            "input_tokens",
            "promptTokens",
            "prompt_tokens",
        ],
    );
    usage.output = pick_number(
        value,
        &[
            "outputTokens",
            "output_tokens",
            "completionTokens",
            "completion_tokens",
        ],
    );
    usage.cache_read = pick_number(
        value,
        &[
            "cacheReadInputTokens",
            "cache_read_input_tokens",
            "cachedTokensCreated",
            "cached_tokens_created",
        ],
    );
    if usage.cache_read == 0.0 {
        let details = value
            .get("promptTokensDetails")
            .or_else(|| value.get("prompt_tokens_details"));
        if let Some(details) = details
            && is_record(details)
        {
            usage.cache_read = pick_number(details, &["cachedTokens", "cached_tokens"]);
        }
    }
    usage.cache_write = pick_number(
        value,
        &[
            "cacheCreationInputTokens",
            "cache_creation_input_tokens",
            "cacheCreationTokens",
            "cache_creation_tokens",
        ],
    );
    if let Some(model) = value.get("model").and_then(as_string)
        && !model.is_empty()
    {
        usage.model = Some(model);
    }
    usage
}

/// OpenRouter-routed calls land their final token counts in the stashed
/// RunState message history rather than metadata.usage. Faithful port of
/// `usageFromRunState`.
fn usage_from_run_state(metadata: &Value) -> Option<Usage> {
    let history = metadata
        .get("runState")
        .filter(|v| is_record(v))?
        .get("sessionState")
        .filter(|v| is_record(v))?
        .get("mainAgentState")
        .filter(|v| is_record(v))?
        .get("messageHistory")?;
    let history = match history {
        Value::Array(a) => a,
        _ => return None,
    };
    let mut accumulator = empty_usage();
    let mut found_any = false;
    for entry in history.iter().rev() {
        if !is_record(entry) || entry.get("role") != Some(&Value::String("assistant".to_owned())) {
            continue;
        }
        let provider_options = match entry.get("providerOptions") {
            Some(p) if is_record(p) => p,
            _ => continue,
        };
        let mut entry_usage = empty_usage();
        merge_fallback(
            &mut entry_usage,
            &parse_usage_object(provider_options.get("usage")),
        );
        if let Some(codebuff_options) = provider_options.get("codebuff")
            && is_record(codebuff_options)
        {
            merge_fallback(
                &mut entry_usage,
                &parse_usage_object(codebuff_options.get("usage")),
            );
            if let Some(model) = codebuff_options.get("model").and_then(as_string)
                && !model.is_empty()
            {
                entry_usage.model = Some(model);
            }
        }
        if has_signal(&entry_usage) || entry_usage.model.is_some() {
            found_any = true;
        }
        merge_fallback(&mut accumulator, &entry_usage);
    }
    if found_any { Some(accumulator) } else { None }
}

/// Faithful port of `extractUsage`.
fn extract_usage(msg: &Value) -> Usage {
    let mut usage = empty_usage();
    let metadata = match msg.get("metadata") {
        Some(m) if is_record(m) => m,
        _ => return usage,
    };
    if let Some(model) = metadata.get("model").and_then(as_string)
        && !model.is_empty()
    {
        usage.model = Some(model);
    }
    merge_fallback(&mut usage, &parse_usage_object(metadata.get("usage")));
    if let Some(codebuff_meta) = metadata.get("codebuff")
        && is_record(codebuff_meta)
    {
        merge_fallback(&mut usage, &parse_usage_object(codebuff_meta.get("usage")));
    }
    if let Some(run_state_usage) = usage_from_run_state(metadata) {
        merge_fallback(&mut usage, &run_state_usage);
    }
    usage
}

/// Faithful port of `isAssistant`.
fn is_assistant(msg: &Value) -> bool {
    let variant = msg
        .get("variant")
        .and_then(as_string)
        .or_else(|| msg.get("role").and_then(as_string))
        .unwrap_or_default();
    variant == "ai" || variant == "agent" || variant == "assistant"
}

/// chatId is the chat's ISO timestamp with the time-portion colons flipped to
/// dashes for filesystem safety; only the two separators after T flip back.
/// Faithful port of `chatIdTimestamp`.
fn chat_id_timestamp(chat_id: &str) -> Option<String> {
    let t_index = chat_id.find('T')?;
    let date = &chat_id[..t_index];
    let mut time = chat_id[t_index..].to_owned();
    for _ in 0..2 {
        // replace the first '-' occurrence with ':', matching String.replace.
        if let Some(pos) = time.find('-') {
            time.replace_range(pos..pos + 1, ":");
        }
    }
    reparse_iso(&format!("{date}{time}"))
}

/// Faithful port of `messageTimestamp`.
fn message_timestamp(msg: &Value) -> Option<String> {
    for key in ["timestamp", "createdAt"] {
        let value = msg.get(key).unwrap_or(&Value::Null);
        if let Some(iso) = epoch_to_iso(value) {
            return Some(iso);
        }
        if let Some(text) = as_string(value)
            && let Some(iso) = reparse_iso(&text)
        {
            return Some(iso);
        }
    }
    if let Some(metadata) = msg.get("metadata")
        && is_record(metadata)
        && let Some(iso) = epoch_to_iso(metadata.get("timestamp").unwrap_or(&Value::Null))
    {
        return Some(iso);
    }
    None
}

/// The parent directory path (node `dirname`), `/`-normalized.
fn dirname(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rfind('/') {
        Some(i) => normalized[..i].to_owned(),
        None => String::new(),
    }
}

/// Read all codebuff usage events. Faithful port of `readCodebuff`.
pub fn read_codebuff(roots: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for root in roots {
        let root_str = root.to_string_lossy().into_owned();
        for file in list_files(root, |name| name == "chat-messages.json") {
            let parsed = match read_json(&file) {
                Some(v) => v,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping unreadable file {file}\n"),
                    });
                    skipped.push(file.clone());
                    continue;
                }
            };
            let messages = match parsed {
                Value::Array(a) => a,
                _ => continue,
            };
            let chat_id = basename(&dirname(&file));
            let project_name = basename(&dirname(&dirname(&dirname(&file))));
            let root_component = if basename(&root_str) == "projects" {
                basename(&dirname(&root_str))
            } else {
                basename(&root_str)
            };
            let session_id = format!("{root_component}/{project_name}/{chat_id}");
            let chat_timestamp = chat_id_timestamp(&chat_id);
            let fallback_timestamp = file_mtime_iso(&file);
            for (ordinal, msg) in messages.iter().enumerate() {
                if !is_record(msg) || !is_assistant(msg) {
                    continue;
                }
                let usage = extract_usage(msg);
                if !has_signal(&usage) {
                    continue;
                }
                let timestamp = message_timestamp(msg)
                    .or_else(|| chat_timestamp.clone())
                    .unwrap_or_else(|| fallback_timestamp.clone());
                let model = usage
                    .model
                    .clone()
                    .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
                let tokens = TokenCounts {
                    input: usage.input as u64,
                    output: usage.output as u64,
                    cache_read: usage.cache_read as u64,
                    cache_write: usage.cache_write as u64,
                    cache_write1h: None,
                    reasoning: 0,
                };
                let upstream_id = msg.get("id").and_then(as_string);
                let key = match upstream_id {
                    Some(id) if !id.is_empty() => id,
                    _ => format!(
                        "codebuff:{}:{}:{}:{}:{}:{}:{}:{}",
                        session_id,
                        timestamp,
                        model,
                        ordinal,
                        tokens.input,
                        tokens.output,
                        tokens.cache_read,
                        tokens.cache_write
                    ),
                };
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key.clone());
                events.push(UsageEvent {
                    harness: "codebuff".to_owned(),
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
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

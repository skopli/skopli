use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use serde_json::Value;

use super::gemini_core::{GEMINI_TOKEN_FIELDS, parse_gemini_tokens};
use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, as_string, dir_exists, file_mtime_iso, is_record,
    list_files, read_json, read_jsonl_lines,
};
use crate::types::UsageEvent;

/// The gemini reader wired into the harness registry.
pub struct GeminiReader;

impl Reader for GeminiReader {
    fn harness_id(&self) -> &'static str {
        "gemini"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = gemini_root(ctx.env("GEMINI_DATA_DIR"), ctx.home());
        read_gemini(&root)
    }
}

/// Resolve the gemini data root. Faithful port of `geminiRoot`.
pub fn gemini_root(config_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(override_val) = config_dir
        && !override_val.is_empty()
    {
        return PathBuf::from(override_val);
    }
    home.join(".gemini")
}

struct FileContext {
    file: String,
    session_id: String,
    fallback_model: Option<String>,
    fallback_timestamp: String,
}

struct ToEvent {
    event: Option<UsageEvent>,
    malformed: bool,
}

/// Faithful port of `toEvent`.
fn to_event(raw: &Value, index: usize, ctx: &FileContext, turn: bool) -> ToEvent {
    if !is_record(raw) {
        return ToEvent {
            event: None,
            malformed: true,
        };
    }
    let tokens = raw.get("tokens");
    let tokens = match tokens {
        Some(t) if is_record(t) => t,
        _ => {
            let malformed = raw.get("type").and_then(Value::as_str) == Some("gemini")
                || raw.get("tokens").is_some();
            return ToEvent {
                event: None,
                malformed,
            };
        }
    };
    let model = raw
        .get("model")
        .and_then(as_string)
        .or_else(|| ctx.fallback_model.clone());
    let parsed_tokens = parse_gemini_tokens(tokens, &GEMINI_TOKEN_FIELDS);
    let (model, parsed_tokens) = match (model, parsed_tokens) {
        (Some(m), Some(t)) => (m, t),
        _ => {
            return ToEvent {
                event: None,
                malformed: true,
            };
        }
    };
    ToEvent {
        event: Some(UsageEvent {
            harness: "gemini".to_owned(),
            timestamp: raw
                .get("timestamp")
                .and_then(as_string)
                .unwrap_or_else(|| ctx.fallback_timestamp.clone()),
            session_id: ctx.session_id.clone(),
            message_id: raw
                .get("id")
                .and_then(as_string)
                .unwrap_or_else(|| format!("{}:{index}", ctx.file)),
            turn,
            subagent: false,
            model,
            tokens: parsed_tokens,
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        }),
        malformed: false,
    }
}

/// Faithful port of `collectEvents`.
fn collect_events(
    messages: &[(usize, Value)],
    ctx: &FileContext,
    events: &mut Vec<UsageEvent>,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) {
    let mut prev_was_user = false;
    for (index, value) in messages {
        if is_record(value) && value.get("type").and_then(Value::as_str) == Some("user") {
            prev_was_user = true;
            continue;
        }
        let result = to_event(value, *index, ctx, prev_was_user);
        if result.malformed {
            let location = format!("{}:{}", ctx.file, index + 1);
            warnings.push(ReaderWarning {
                message: format!("skipping malformed gemini record {location}\n"),
            });
            skipped.push(location);
        } else if let Some(event) = result.event {
            events.push(event);
            prev_was_user = false;
        }
    }
}

struct Extracted {
    messages: Vec<Value>,
    model: Option<String>,
    session_id: Option<String>,
}

/// Faithful port of `extractMessages`.
fn extract_messages(parsed: &Value) -> Extracted {
    if let Value::Array(arr) = parsed {
        return Extracted {
            messages: arr.clone(),
            model: None,
            session_id: None,
        };
    }
    if is_record(parsed) {
        let model = parsed.get("model").and_then(as_string);
        let session_id = parsed.get("sessionId").and_then(as_string);
        if let Some(Value::Array(messages)) = parsed.get("messages") {
            return Extracted {
                messages: messages.clone(),
                model,
                session_id,
            };
        }
    }
    Extracted {
        messages: Vec::new(),
        model: None,
        session_id: None,
    }
}

/// Read all gemini usage events from the given root. Faithful port of `readGemini`.
pub fn read_gemini(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let tmp = root.join("tmp");
    if !dir_exists(&tmp) {
        return ReaderResult {
            events,
            skipped,
            warnings,
        };
    }
    let chats_marker = format!("{MAIN_SEPARATOR}chats{MAIN_SEPARATOR}");
    let files: Vec<String> = list_files(&tmp, |name| {
        name.ends_with(".json") || name.ends_with(".jsonl")
    })
    .into_iter()
    .filter(|file| file.contains(&chats_marker))
    .collect();

    for file in files {
        let fallback_timestamp = file_mtime_iso(&file);
        if file.ends_with(".jsonl") {
            let lines = match read_jsonl_lines(&file, &mut skipped, &mut warnings) {
                Some(l) => l,
                None => continue,
            };
            let msgs: Vec<(usize, Value)> = lines
                .into_iter()
                .map(|JsonlLine { index, value }| (index, value))
                .collect();
            collect_events(
                &msgs,
                &FileContext {
                    file: file.clone(),
                    session_id: file.clone(),
                    fallback_model: None,
                    fallback_timestamp,
                },
                &mut events,
                &mut skipped,
                &mut warnings,
            );
            continue;
        }
        let parsed = match read_json(&file) {
            Some(p) => p,
            None => {
                warnings.push(ReaderWarning {
                    message: format!("skipping unreadable gemini chat {file}\n"),
                });
                skipped.push(file.clone());
                continue;
            }
        };
        let Extracted {
            messages,
            model,
            session_id,
        } = extract_messages(&parsed);
        let indexed: Vec<(usize, Value)> = messages.into_iter().enumerate().collect();
        collect_events(
            &indexed,
            &FileContext {
                file: file.clone(),
                session_id: session_id.unwrap_or_else(|| file.clone()),
                fallback_model: model,
                fallback_timestamp,
            },
            &mut events,
            &mut skipped,
            &mut warnings,
        );
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

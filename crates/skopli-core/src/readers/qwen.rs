use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::gemini_core::{QWEN_TOKEN_FIELDS, parse_gemini_tokens};
use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, as_string, basename, file_mtime_iso, is_record,
    list_files, parent_dir_name, read_jsonl_lines, reparse_iso,
};
use crate::types::UsageEvent;

/// The qwen reader wired into the harness registry.
pub struct QwenReader;

impl Reader for QwenReader {
    fn harness_id(&self) -> &'static str {
        "qwen"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = qwen_projects_root(ctx.env("QWEN_DATA_DIR"), ctx.home());
        read_qwen(&root)
    }
}

/// Resolve the qwen projects root. Faithful port of `qwenProjectsRoot`.
pub fn qwen_projects_root(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v).join("projects");
    }
    home.join(".qwen").join("projects")
}

/// Faithful port of `sessionId(raw, file)`: prefer `raw.sessionId`, else
/// `<grandparent dir name>-<file stem>`.
fn session_id(raw: &Value, file: &str) -> String {
    raw.get("sessionId").and_then(as_string).unwrap_or_else(|| {
        let grandparent = parent_dir_name_full(file);
        let stem = file_stem_name_jsonl(file);
        format!("{grandparent}-{stem}")
    })
}

/// `basename(dirname(dirname(file)))` -> grandparent directory name.
fn parent_dir_name_full(file: &str) -> String {
    // parent_dir_name gives basename(dirname(path)); apply twice by trimming.
    let normalized = file.replace('\\', "/");
    let up_one = match normalized.rfind('/') {
        Some(i) => &normalized[..i],
        None => "",
    };
    parent_dir_name(up_one)
}

/// `basename(file, ".jsonl")` -> file name without a trailing `.jsonl`.
fn file_stem_name_jsonl(file: &str) -> String {
    let name = basename(file);
    name.strip_suffix(".jsonl").unwrap_or(&name).to_owned()
}

enum Role {
    User,
    Assistant,
}

struct QwenMessage {
    event: Option<UsageEvent>,
    role: Role,
    session_id: String,
}

/// Read all qwen usage events. Faithful port of `readQwen`.
pub fn read_qwen(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut by_session: HashMap<String, Vec<QwenMessage>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for file in list_files(root, |name| name.ends_with(".jsonl")) {
        let lines = match read_jsonl_lines(&file, &mut skipped, &mut warnings) {
            Some(l) => l,
            None => continue,
        };
        for JsonlLine { index, value } in &lines {
            let message = parse_message(value, &file, *index, &mut skipped, &mut warnings);
            if let Some(msg) = message {
                let sid = msg.session_id.clone();
                let entry = by_session.entry(sid.clone()).or_insert_with(|| {
                    order.push(sid.clone());
                    Vec::new()
                });
                entry.push(msg);
            }
        }
    }

    for sid in &order {
        let messages = match by_session.get(sid) {
            Some(m) => m,
            None => continue,
        };
        let mut previous_was_user = false;
        for message in messages {
            match message.role {
                Role::User => previous_was_user = true,
                Role::Assistant => {
                    if let Some(event) = &message.event {
                        let mut event = event.clone();
                        event.turn = previous_was_user;
                        previous_was_user = false;
                        events.push(event);
                    }
                }
            }
        }
    }

    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

fn parse_message(
    value: &Value,
    file: &str,
    index: usize,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Option<QwenMessage> {
    if !is_record(value) {
        return None;
    }
    let role = value.get("type").and_then(as_string);
    match role.as_deref() {
        Some("user") => {
            return Some(QwenMessage {
                role: Role::User,
                session_id: session_id(value, file),
                event: None,
            });
        }
        Some("assistant") => {}
        _ => return None,
    }
    let usage = value.get("usageMetadata").unwrap_or(&Value::Null);
    let tokens = parse_gemini_tokens(usage, &QWEN_TOKEN_FIELDS);
    let tokens = match tokens {
        Some(t) => t,
        None => {
            let location = format!("{file}:{}", index + 1);
            skipped.push(location.clone());
            warnings.push(ReaderWarning {
                message: format!("skipping malformed qwen record {location}\n"),
            });
            return None;
        }
    };
    if tokens.input + tokens.output + tokens.cache_read + tokens.reasoning == 0 {
        return None;
    }
    let timestamp = value
        .get("timestamp")
        .and_then(as_string)
        .and_then(|t| reparse_iso(&t))
        .unwrap_or_else(|| file_mtime_iso(file));
    Some(QwenMessage {
        role: Role::Assistant,
        session_id: session_id(value, file),
        event: Some(UsageEvent {
            harness: "qwen".to_owned(),
            timestamp,
            session_id: session_id(value, file),
            message_id: format!("{file}:{index}"),
            turn: false,
            subagent: false,
            model: value
                .get("model")
                .and_then(as_string)
                .unwrap_or_else(|| "unknown".to_owned()),
            tokens,
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        }),
    })
}

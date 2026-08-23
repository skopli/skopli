use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, basename, dir_exists, finite_number, is_record,
    list_files, parse_timestamp_to_iso, read_json,
};
use crate::types::{TokenCounts, UsageEvent};

/// The roo reader wired into the harness registry. roo is one of three VS Code
/// task-store extensions (roo/cline/kilocode) that share a per-OS globalStorage
/// layout; only the extension-id suffix varies. This reader serves the `roo`
/// harness (`rooveterinaryinc.roo-cline`).
pub struct RooReader;

impl Reader for RooReader {
    fn harness_id(&self) -> &'static str {
        "roo"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = roo_task_roots(ctx.env("APPDATA"), ctx.home());
        read_roo(&roots)
    }
}

/// Resolve the roo VS Code task roots. Faithful port of `vscodeTaskRoots("roo")`:
/// the per-OS globalStorage roots for the `rooveterinaryinc.roo-cline/tasks`
/// extension suffix, plus an `%APPDATA%` root when that env var is set.
pub fn roo_task_roots(app_data: Option<&str>, home: &Path) -> Vec<PathBuf> {
    let suffix = ["rooveterinaryinc.roo-cline", "tasks"];
    let with_suffix = |base: PathBuf| -> PathBuf {
        let mut p = base;
        for part in suffix {
            p = p.join(part);
        }
        p
    };
    let mut roots = vec![
        with_suffix(
            home.join(".config")
                .join("Code")
                .join("User")
                .join("globalStorage"),
        ),
        with_suffix(
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("globalStorage"),
        ),
        with_suffix(
            home.join(".vscode-server")
                .join("data")
                .join("User")
                .join("globalStorage"),
        ),
        with_suffix(
            home.join("AppData")
                .join("Roaming")
                .join("Code")
                .join("User")
                .join("globalStorage"),
        ),
    ];
    if let Some(app_data) = app_data
        && !app_data.is_empty()
    {
        roots.push(with_suffix(
            PathBuf::from(app_data)
                .join("Code")
                .join("User")
                .join("globalStorage"),
        ));
    }
    roots
}

/// Faithful port of `numeric`: a finite number (clamped `>= 0`), a finite
/// numeric string, else 0.
fn numeric(value: Option<&Value>) -> u64 {
    let value = match value {
        Some(v) => v,
        None => return 0,
    };
    if let Some(parsed) = finite_number(value) {
        return parsed.max(0.0) as u64;
    }
    if let Some(text) = value.as_str()
        && let Ok(from_string) = text.parse::<f64>()
        && from_string.is_finite()
    {
        return from_string.max(0.0) as u64;
    }
    0
}

/// Faithful port of `tagValue`: the trimmed contents of the LAST `<tag>...</tag>`
/// in `text`, or `None`.
fn tag_value(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut last: Option<String> = None;
    let mut search_from = 0;
    while let Some(rel_open) = text[search_from..].find(&open) {
        let content_start = search_from + rel_open + open.len();
        match text[content_start..].find(&close) {
            Some(rel_close) => {
                let content_end = content_start + rel_close;
                last = Some(text[content_start..content_end].to_owned());
                search_from = content_end + close.len();
            }
            None => break,
        }
    }
    last.map(|s| s.trim().to_owned())
}

/// model comes from the sibling api_conversation_history.json: the last `<model>`
/// value inside any `<environment_details>` block wins. Faithful port of
/// `modelForTask`.
fn model_for_task(task_dir: &str) -> String {
    let sibling = format!("{task_dir}/api_conversation_history.json");
    let text = match fs::read_to_string(&sibling) {
        Ok(t) => t,
        Err(_) => return "unknown".to_owned(),
    };
    let mut model: Option<String> = None;
    let open = "<environment_details>";
    let close = "</environment_details>";
    let mut search_from = 0;
    while let Some(rel_open) = text[search_from..].find(open) {
        let block_start = search_from + rel_open;
        match text[block_start..].find(close) {
            Some(rel_close) => {
                let block_end = block_start + rel_close + close.len();
                let block = &text[block_start..block_end];
                if let Some(found) = tag_value(block, "model")
                    && !found.is_empty()
                {
                    model = Some(found);
                }
                search_from = block_end;
            }
            None => break,
        }
    }
    model.unwrap_or_else(|| "unknown".to_owned())
}

/// The parent directory path (node `dirname`), `/`-normalized.
fn dirname(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rfind('/') {
        Some(i) => normalized[..i].to_owned(),
        None => String::new(),
    }
}

/// Read a single task's `ui_messages.json`. Faithful port of `readTaskFile`
/// specialized to the `roo` harness.
fn read_task_file(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let entries = match read_json(file) {
        Some(v) => v,
        None => {
            warnings.push(ReaderWarning {
                message: format!("skipping unreadable roo task {file}\n"),
            });
            skipped.push(file.to_owned());
            return Vec::new();
        }
    };
    let entries = match entries {
        Value::Array(a) => a,
        _ => return Vec::new(),
    };
    let task_dir = dirname(file);
    let session_id = {
        let base = basename(&task_dir);
        if base.is_empty() {
            "unknown".to_owned()
        } else {
            base
        }
    };
    let model = model_for_task(&task_dir);
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut index: usize = 0;
    for entry in &entries {
        if !is_record(entry) {
            continue;
        }
        if entry.get("type").and_then(as_string).as_deref() != Some("say")
            || entry.get("say").and_then(as_string).as_deref() != Some("api_req_started")
        {
            continue;
        }
        let timestamp = match parse_timestamp_to_iso(entry.get("ts").unwrap_or(&Value::Null)) {
            Some(t) => t,
            None => continue,
        };
        let text_raw = match entry.get("text").and_then(as_string) {
            Some(t) => t,
            None => continue,
        };
        let payload: Value = match serde_json::from_str(&text_raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !is_record(&payload) {
            continue;
        }
        let tokens = TokenCounts {
            input: numeric(payload.get("tokensIn")),
            output: numeric(payload.get("tokensOut")),
            cache_read: numeric(payload.get("cacheReads")),
            cache_write: numeric(payload.get("cacheWrites")),
            cache_write1h: None,
            reasoning: 0,
        };
        if tokens.input == 0
            && tokens.output == 0
            && tokens.cache_read == 0
            && tokens.cache_write == 0
        {
            index += 1;
            continue;
        }
        let provider = {
            let raw = payload
                .get("apiProtocol")
                .and_then(as_string)
                .map(|s| s.trim().to_owned())
                .unwrap_or_default();
            if raw.is_empty() {
                "unknown".to_owned()
            } else {
                raw
            }
        };
        events.push(UsageEvent {
            harness: "roo".to_owned(),
            timestamp,
            session_id: session_id.clone(),
            message_id: format!("roo:{session_id}:{index}"),
            turn: false,
            subagent: false,
            model: format!("{provider}/{model}"),
            tokens,
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        });
        index += 1;
    }
    events
}

/// Read all roo usage events. Faithful port of `readRoo` -> `readVscodeExt`.
pub fn read_roo(roots: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for root in roots {
        if !dir_exists(root) {
            continue;
        }
        for file in list_files(root, |name| name == "ui_messages.json") {
            for event in read_task_file(&file, &mut skipped, &mut warnings) {
                if seen.contains(&event.message_id) {
                    continue;
                }
                seen.insert(event.message_id.clone());
                events.push(event);
            }
        }
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

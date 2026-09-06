use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, basename, dir_exists, file_mtime_iso, is_record,
    list_files, parse_timestamp_to_iso, read_json,
};
use super::vscode_ext::{numeric, read_vscode_ext, vscode_task_roots};
use crate::types::{TokenCounts, UsageEvent};

/// The cline reader wired into the harness registry. cline ships two lanes: a
/// VS Code extension task store (`saoudrizwan.claude-dev`) and a CLI session
/// store; the extension store is authoritative and suppresses the matching CLI
/// mirror wholesale. Faithful port of `readCline` in roo-cline-kilo.ts.
pub struct ClineReader;

impl Reader for ClineReader {
    fn harness_id(&self) -> &'static str {
        "cline"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let ext_roots = cline_task_roots(ctx.env("APPDATA"), ctx.home());
        let cli_roots = cline_cli_session_roots(
            ctx.env("CLINE_SESSION_DATA_DIR"),
            ctx.env("CLINE_DATA_DIR"),
            ctx.env("CLINE_DIR"),
            ctx.home(),
        );
        read_cline(&ext_roots, &cli_roots)
    }
}

/// Resolve the cline VS Code extension task roots. Faithful port of
/// `clineTaskRoots` -> `vscodeTaskRoots("cline")`.
pub fn cline_task_roots(app_data: Option<&str>, home: &Path) -> Vec<PathBuf> {
    vscode_task_roots(app_data, home, &["saoudrizwan.claude-dev", "tasks"])
}

/// Resolve the cline CLI session roots. Faithful port of `clineCliSessionRoots`:
/// `CLINE_SESSION_DATA_DIR` -> `CLINE_DATA_DIR/sessions` -> `CLINE_DIR/data/sessions`
/// -> `~/.cline/data/sessions` (first non-empty override wins).
pub fn cline_cli_session_roots(
    session_data_dir: Option<&str>,
    data_dir: Option<&str>,
    cline_dir: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(direct) = session_data_dir {
        let trimmed = direct.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(trimmed)];
        }
    }
    if let Some(data_dir) = data_dir {
        let trimmed = data_dir.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(trimmed).join("sessions")];
        }
    }
    if let Some(cline_dir) = cline_dir {
        let trimmed = cline_dir.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(trimmed).join("data").join("sessions")];
        }
    }
    vec![home.join(".cline").join("data").join("sessions")]
}

/// Faithful port of `isHumanPrompt`: an array content with at least one `text`
/// block and no `tool_result` block.
fn is_human_prompt(content: Option<&Value>) -> bool {
    let content = match content {
        Some(Value::Array(a)) => a,
        _ => return false,
    };
    let mut has_text = false;
    for block in content {
        if !is_record(block) {
            continue;
        }
        match block.get("type").and_then(Value::as_str) {
            Some("tool_result") => return false,
            Some("text") => has_text = true,
            _ => {}
        }
    }
    has_text
}

/// Read the cline CLI session lane. Faithful port of `readClineCli`.
fn read_cline_cli(
    roots: &[PathBuf],
    seen: &mut HashSet<String>,
    covered_sessions: &HashSet<String>,
) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    for root in roots {
        if !dir_exists(root) {
            continue;
        }
        for file in list_files(root, |name| name.ends_with(".messages.json")) {
            let parsed = match read_json(&file) {
                Some(v) => v,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping unreadable cline session {file}\n"),
                    });
                    skipped.push(file.clone());
                    continue;
                }
            };
            if !is_record(&parsed) {
                continue;
            }
            let file_base = basename(&file);
            let stem = file_base
                .strip_suffix(".messages.json")
                .unwrap_or(&file_base);
            let manifest_path = {
                let dir = dirname(&file);
                if dir.is_empty() {
                    format!("{stem}.json")
                } else {
                    format!("{dir}/{stem}.json")
                }
            };
            let manifest = match read_json(&manifest_path) {
                Some(v) if is_record(&v) => v,
                _ => Value::Object(serde_json::Map::new()),
            };
            let mut current_model = manifest.get("model").and_then(as_string);
            let mut current_provider = manifest.get("provider").and_then(as_string);
            let session_id = parsed
                .get("sessionId")
                .and_then(as_string)
                .unwrap_or_else(|| stem.to_owned());
            // the extension store is authoritative; a session it already emitted
            // is a CLI mirror of the same work and is suppressed wholesale
            if covered_sessions.contains(&session_id) {
                continue;
            }
            let messages = match parsed.get("messages") {
                Some(Value::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            let mut pending_turn = false;
            let mut assistant_index: u64 = 0;
            for message in &messages {
                if !is_record(message) {
                    continue;
                }
                let role = message.get("role").and_then(as_string);
                if role.as_deref() == Some("user") {
                    if is_human_prompt(message.get("content")) {
                        pending_turn = true;
                    }
                    continue;
                }
                if role.as_deref() != Some("assistant") {
                    continue;
                }
                if let Some(model_info) = message.get("modelInfo").filter(|v| is_record(v)) {
                    if let Some(id) = model_info.get("id").and_then(as_string) {
                        let trimmed = id.trim();
                        if !trimmed.is_empty() {
                            current_model = Some(trimmed.to_owned());
                        }
                    }
                    if let Some(provider) = model_info.get("provider").and_then(as_string) {
                        let trimmed = provider.trim();
                        if !trimmed.is_empty() {
                            current_provider = Some(trimmed.to_owned());
                        }
                    }
                }
                let id_field = message.get("id").and_then(as_string);
                let dedup_key = match id_field.as_deref().map(str::trim) {
                    Some(id) if !id.is_empty() => {
                        format!("cline-cli:{session_id}:{id}")
                    }
                    _ => format!("cline-cli:{session_id}:{assistant_index}"),
                };
                assistant_index += 1;
                let metrics = match message.get("metrics").filter(|v| is_record(v)) {
                    Some(m) => m,
                    // a dropped assistant row must not consume the pending turn
                    None => continue,
                };
                let input_inclusive = numeric(metrics.get("inputTokens"));
                let cache_read = numeric(metrics.get("cacheReadTokens"));
                let cache_write = numeric(metrics.get("cacheWriteTokens"));
                let tokens = TokenCounts {
                    input: input_inclusive.saturating_sub(cache_read + cache_write),
                    output: numeric(metrics.get("outputTokens")),
                    cache_read,
                    cache_write,
                    cache_write1h: None,
                    reasoning: 0,
                };
                let total_tokens =
                    tokens.input + tokens.output + tokens.cache_read + tokens.cache_write;
                // only real persisted token counts drive usage; a positive
                // vendor cost must never make a zero-token row count as a call
                if total_tokens == 0 {
                    continue;
                }
                if seen.contains(&dedup_key) {
                    continue;
                }
                seen.insert(dedup_key.clone());
                let turn = pending_turn;
                pending_turn = false;
                let provider = match current_provider.as_deref() {
                    Some(p) if !p.is_empty() => p.to_owned(),
                    _ => "cline".to_owned(),
                };
                let model = match current_model.as_deref() {
                    Some(m) if !m.is_empty() => m.to_owned(),
                    _ => "unknown".to_owned(),
                };
                let timestamp = parse_timestamp_to_iso(message.get("ts").unwrap_or(&Value::Null))
                    .unwrap_or_else(|| file_mtime_iso(&file));
                events.push(UsageEvent {
                    harness: "cline".to_owned(),
                    timestamp,
                    session_id: session_id.clone(),
                    message_id: dedup_key,
                    turn,
                    subagent: false,
                    model: format!("{provider}/{model}"),
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

/// The parent directory path (node `dirname`), `/`-normalized.
fn dirname(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rfind('/') {
        Some(i) => normalized[..i].to_owned(),
        None => String::new(),
    }
}

/// Read all cline usage events. Faithful port of `readCline`: the extension
/// store is authoritative (any session it emits suppresses the matching CLI
/// mirror wholesale, and `seen` catches within-store duplicates).
pub fn read_cline(ext_roots: &[PathBuf], cli_roots: &[PathBuf]) -> ReaderResult {
    let mut seen: HashSet<String> = HashSet::new();
    let mut covered_sessions: HashSet<String> = HashSet::new();
    let ext = read_vscode_ext("cline", ext_roots, &mut seen, &mut covered_sessions);
    let cli = read_cline_cli(cli_roots, &mut seen, &covered_sessions);
    let mut events = ext.events;
    events.extend(cli.events);
    let mut skipped = ext.skipped;
    skipped.extend(cli.skipped);
    let mut warnings = ext.warnings;
    warnings.extend(cli.warnings);
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

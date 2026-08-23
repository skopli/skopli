use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    EpochUnit, JsonlLine, ReaderResult, ReaderWarning, ScanJsonl, as_string, epoch_to_iso_unit,
    file_mtime_iso, finite_number, is_record, list_files, parent_dir_name, scan_jsonl,
};
use crate::types::{TokenCounts, UsageEvent};

/// The junie reader wired into the harness registry. Faithful port of
/// `readJunie` in src/readers/junie.ts.
pub struct JunieReader;

impl Reader for JunieReader {
    fn harness_id(&self) -> &'static str {
        "junie"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = junie_sessions_root(ctx.env("JUNIE_SESSIONS_DIR"), ctx.home());
        read_junie(&root)
    }
}

/// Resolve the junie sessions root. Faithful port of `junieSessionsRoot`:
/// `JUNIE_SESSIONS_DIR` wins; else `~/.junie/sessions`.
pub fn junie_sessions_root(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(override_val) = override_dir
        && !override_val.is_empty()
    {
        return PathBuf::from(override_val);
    }
    home.join(".junie").join("sessions")
}

const USAGE_KIND: &str = "LlmResponseMetadataEvent";
const USER_PROMPT_KIND: &str = "UserPromptEvent";
const SKIP_EVENT_KINDS: [&str; 3] = [
    "AgentStateUpdatedEvent",
    "AgentCurrentStatusUpdatedEvent",
    "AgentPatchCreatedEvent",
];

/// Faithful port of `topLevelKind`.
fn top_level_kind(record: &Value) -> Option<String> {
    let kind = record.get("kind").and_then(as_string)?;
    if kind.trim().is_empty() {
        None
    } else {
        Some(kind)
    }
}

/// Faithful port of `parsedEventKind`: prefer top-level `kind`, fall back to
/// `event.agentEvent.kind`.
fn parsed_event_kind(record: &Value) -> Option<String> {
    if let Some(top) = top_level_kind(record) {
        return Some(top);
    }
    let event_value = record.get("event")?;
    if !is_record(event_value) {
        return None;
    }
    let agent_event = event_value.get("agentEvent")?;
    if !is_record(agent_event) {
        return None;
    }
    agent_event.get("kind").and_then(as_string)
}

/// Faithful port of `pick` in junie.ts: the first finite key, clamped `>= 0`.
fn pick(value: &Value, keys: &[&str]) -> u64 {
    for key in keys {
        if let Some(found) = value.get(*key).and_then(finite_number) {
            return found.max(0.0) as u64;
        }
    }
    0
}

/// Faithful port of `usageTokens` in junie.ts.
fn usage_tokens(value: &Value) -> Option<TokenCounts> {
    let tokens = TokenCounts {
        input: pick(value, &["inputTokens", "input"]),
        output: pick(value, &["outputTokens", "output"]),
        cache_read: pick(
            value,
            &["cacheInputTokens", "cacheReadInputTokens", "cacheRead"],
        ),
        cache_write: pick(value, &["cacheCreateTokens", "cacheCreationInputTokens"]),
        cache_write1h: None,
        reasoning: pick(
            value,
            &["reasoningTokens", "reasoningOutputTokens", "thinkingTokens"],
        ),
    };
    let any = tokens.input > 0
        || tokens.output > 0
        || tokens.cache_read > 0
        || tokens.cache_write > 0
        || tokens.reasoning > 0;
    if any { Some(tokens) } else { None }
}

struct JunieEntry {
    event: UsageEvent,
}

struct Row {
    usage_index: usize,
    model: String,
    tokens: TokenCounts,
    key: String,
}

/// Read all junie usage events. Faithful port of `readJunie`.
pub fn read_junie(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let files = list_files(root, |name| name == "events.jsonl");
    let mut seen: HashSet<String> = HashSet::new();

    for file in &files {
        let session_id = parent_dir_name(file);
        let fallback_timestamp = file_mtime_iso(file);
        let mut pending_turn = false;

        let files_slice = std::slice::from_ref(file);
        let mut parse = |line: &JsonlLine, file: &str| -> Option<Vec<JunieEntry>> {
            if !is_record(&line.value) {
                return None;
            }
            let record = &line.value;
            let event_kind = parsed_event_kind(record);
            if let Some(kind) = event_kind.as_deref()
                && SKIP_EVENT_KINDS.contains(&kind)
            {
                return None;
            }
            if event_kind.as_deref() == Some(USER_PROMPT_KIND) {
                pending_turn = true;
                return None;
            }
            if event_kind.as_deref() != Some(USAGE_KIND) {
                return None;
            }
            let usages = record
                .get("event")
                .filter(|v| is_record(v))
                .and_then(|e| e.get("agentEvent"))
                .filter(|v| is_record(v))
                .and_then(|a| a.get("modelUsage"));
            let usages = match usages {
                Some(Value::Array(a)) => a,
                _ => {
                    // a recognised metadata event consumes the pending turn once
                    pending_turn = false;
                    return None;
                }
            };
            let timestamp = epoch_to_iso_unit(
                record.get("timestampMs").unwrap_or(&Value::Null),
                EpochUnit::Ms,
            )
            .unwrap_or_else(|| fallback_timestamp.clone());
            let mut rows: Vec<Row> = Vec::new();
            for (usage_index, usage) in usages.iter().enumerate() {
                if !is_record(usage) {
                    continue;
                }
                let model = match usage.get("model").and_then(as_string) {
                    Some(m) if !m.is_empty() => m,
                    _ => continue,
                };
                let tokens = match usage_tokens(usage) {
                    Some(t) => t,
                    None => continue,
                };
                let tokens_json = serde_json::to_string(&tokens).unwrap_or_default();
                let key =
                    format!("junie:{session_id}:{timestamp}:{model}:{tokens_json}:{usage_index}");
                rows.push(Row {
                    usage_index,
                    model,
                    tokens,
                    key,
                });
            }
            // a wholly-replayed metadata event must not consume the marker
            if !rows.is_empty() && rows.iter().all(|row| seen.contains(&row.key)) {
                return None;
            }
            let mut out: Vec<JunieEntry> = Vec::new();
            let mut turn_assigned = false;
            for row in rows {
                if seen.contains(&row.key) {
                    continue;
                }
                seen.insert(row.key.clone());
                let is_turn_start = pending_turn && !turn_assigned;
                if is_turn_start {
                    turn_assigned = true;
                }
                out.push(JunieEntry {
                    event: UsageEvent {
                        harness: "junie".to_owned(),
                        timestamp: timestamp.clone(),
                        session_id: session_id.clone(),
                        message_id: format!("{file}:{}:{}", line.index, row.usage_index),
                        turn: is_turn_start,
                        subagent: false,
                        model: row.model,
                        tokens: row.tokens,
                        calls: None,
                        cost_usd: None,
                        workspace: None,
                        title: None,
                    },
                });
            }
            pending_turn = false;
            if out.is_empty() { None } else { Some(out) }
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
        for batch in entries {
            for entry in batch {
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

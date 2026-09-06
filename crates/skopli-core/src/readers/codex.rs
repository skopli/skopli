use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_number, as_string, basename, dir_exists, file_mtime_iso,
    is_record, list_files, read_jsonl_lines,
};
use crate::types::{TokenCounts, UsageEvent};

/// The codex reader wired into the harness registry.
pub struct CodexReader;

impl Reader for CodexReader {
    fn harness_id(&self) -> &'static str {
        "codex"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = codex_root(ctx.env("CODEX_HOME"), ctx.home());
        read_codex(&root)
    }
}

/// Resolve the codex data root. Faithful port of `codexRoot`: `CODEX_HOME`
/// override wins, else `~/.codex`.
pub fn codex_root(config_home: Option<&str>, home: &Path) -> PathBuf {
    if let Some(override_val) = config_home
        && !override_val.is_empty()
    {
        return PathBuf::from(override_val);
    }
    home.join(".codex")
}

/// Internal cumulative token snapshot in raw (pre-split) form. Mirrors the TS
/// `TokenCounts` used inside codex.ts before `splitReasoning`. Values stay
/// signed f64 so intermediate deltas can be negative before clamping, exactly
/// like the JS number arithmetic.
#[derive(Clone, Copy)]
struct RawCounts {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    reasoning: f64,
}

/// Faithful port of `usageCounts`.
fn usage_counts(value: &Value) -> Option<RawCounts> {
    if !is_record(value) {
        return None;
    }
    let raw_input = as_number(&value["input_tokens"]);
    // codex input_tokens includes cached input; clamp then subtract to keep
    // input non-cached
    let cached = as_number(&value["cached_input_tokens"])
        .max(as_number(&value["cache_read_input_tokens"]))
        .min(raw_input);
    // codex input_tokens also includes cache-write tokens (2026-07 protocol
    // addition); like cache reads they are a disjoint subset of input, so
    // subtract them too. Absent in older files -> 0.
    let cache_write = as_number(&value["cache_write_input_tokens"]);
    Some(RawCounts {
        input: (raw_input - cached - cache_write).max(0.0),
        output: as_number(&value["output_tokens"]),
        cache_read: cached,
        cache_write,
        reasoning: as_number(&value["reasoning_output_tokens"]),
    })
}

/// codex output_tokens includes reasoning; split the overlap at emission time.
/// Faithful port of `splitReasoning`.
fn split_reasoning(tokens: RawCounts) -> RawCounts {
    let reasoning = tokens.reasoning.max(0.0).min(tokens.output.max(0.0));
    RawCounts {
        output: (tokens.output - reasoning).max(0.0),
        reasoning,
        ..tokens
    }
}

/// Faithful port of `tokenCountUsage`.
fn token_count_usage(payload: &Value) -> (Option<RawCounts>, Option<RawCounts>) {
    let info = match payload.get("info") {
        Some(i) if is_record(i) => i,
        _ => payload,
    };
    (
        info.get("total_token_usage").and_then(usage_counts),
        info.get("last_token_usage").and_then(usage_counts),
    )
}

fn is_zero(t: &RawCounts) -> bool {
    t.input == 0.0
        && t.output == 0.0
        && t.cache_read == 0.0
        && t.cache_write == 0.0
        && t.reasoning == 0.0
}

fn advanced(current: &RawCounts, previous: &RawCounts) -> bool {
    current.input >= previous.input
        && current.output >= previous.output
        && current.cache_read >= previous.cache_read
        && current.cache_write >= previous.cache_write
        && current.reasoning >= previous.reasoning
}

fn same_counts(a: &RawCounts, b: &RawCounts) -> bool {
    a.input == b.input
        && a.output == b.output
        && a.cache_read == b.cache_read
        && a.cache_write == b.cache_write
        && a.reasoning == b.reasoning
}

fn delta(current: &RawCounts, previous: &RawCounts) -> RawCounts {
    RawCounts {
        input: current.input - previous.input,
        output: current.output - previous.output,
        cache_read: current.cache_read - previous.cache_read,
        cache_write: current.cache_write - previous.cache_write,
        reasoning: current.reasoning - previous.reasoning,
    }
}

/// Faithful port of `isUserInput`.
fn is_user_input(line: &Value, payload: &Value) -> bool {
    if line.get("type").and_then(Value::as_str) == Some("event_msg")
        && payload.get("type").and_then(Value::as_str) == Some("user_message")
    {
        return true;
    }
    if line.get("type").and_then(Value::as_str) == Some("response_item")
        && payload.get("type").and_then(Value::as_str) == Some("message")
    {
        return payload.get("role").and_then(Value::as_str) == Some("user");
    }
    false
}

struct Pending {
    timestamp: String,
    index: usize,
    tokens: RawCounts,
    turn: bool,
    model: String,
}

/// Snapshot of usage json for replay-key hashing (mirrors JSON.stringify of the
/// preferred snapshot).
fn snapshot_json(t: &RawCounts) -> String {
    // codex.ts hashes `JSON.stringify(totals ?? last)` where the object is the
    // *raw* {input,output,cacheRead,cacheWrite,reasoning} TokenCounts. Preserve
    // the TS key order and integer formatting.
    format!(
        "{{\"input\":{},\"output\":{},\"cacheRead\":{},\"cacheWrite\":{},\"reasoning\":{}}}",
        fmt_num(t.input),
        fmt_num(t.output),
        fmt_num(t.cache_read),
        fmt_num(t.cache_write),
        fmt_num(t.reasoning),
    )
}

/// Format a number the way `JSON.stringify` would for these integer counters.
fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Faithful port of `readSessionFile`.
fn read_session_file(
    file: &str,
    events: &mut Vec<UsageEvent>,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
    seen_replay_keys: &mut std::collections::HashSet<String>,
) {
    let lines = match read_jsonl_lines(file, skipped, warnings) {
        Some(l) => l,
        None => return,
    };
    let fallback_timestamp = file_mtime_iso(file);
    let mut session_id = {
        let base = basename(file);
        base.strip_suffix(".jsonl").unwrap_or(&base).to_owned()
    };
    let mut model: Option<String> = None;
    let mut previous: Option<RawCounts> = None;
    let mut after_user_input = false;
    let mut pre_context_events = 0usize;
    let mut pending: Vec<Pending> = Vec::new();
    let mut file_replay_keys: Vec<String> = Vec::new();

    for line in &lines {
        let value = &line.value;
        if !is_record(value) {
            continue;
        }
        let payload = match value.get("payload") {
            Some(p) if is_record(p) => p,
            _ => continue,
        };
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            session_id = payload.get("id").and_then(as_string).unwrap_or(session_id);
            continue;
        }
        if value.get("type").and_then(Value::as_str) == Some("turn_context") {
            let model_info = payload.get("model_info");
            let resolved = payload
                .get("model")
                .and_then(as_string)
                .or_else(|| payload.get("model_name").and_then(as_string))
                .or_else(|| {
                    model_info
                        .filter(|m| is_record(m))
                        .and_then(|m| m.get("slug"))
                        .and_then(as_string)
                })
                .or_else(|| model.clone());
            model = resolved;
            continue;
        }
        if is_user_input(value, payload) {
            after_user_input = true;
            continue;
        }
        if value.get("type").and_then(Value::as_str) != Some("event_msg")
            || payload.get("type").and_then(Value::as_str) != Some("token_count")
        {
            continue;
        }
        let (totals, last) = token_count_usage(payload);
        if totals.is_none() && last.is_none() {
            continue;
        }
        let timestamp = value
            .get("timestamp")
            .and_then(as_string)
            .unwrap_or_else(|| fallback_timestamp.clone());
        let snap = totals.as_ref().or(last.as_ref()).expect("one is some");
        let replay_key = format!("{timestamp}:{}", snapshot_json(snap));
        let is_replay = seen_replay_keys.contains(&replay_key);
        file_replay_keys.push(replay_key);

        let stale = match (&totals, &previous) {
            (Some(t), Some(p)) => same_counts(t, p),
            _ => false,
        };
        let mut tokens: Option<RawCounts> = None;
        if let Some(l) = &last
            && !stale
        {
            if !is_zero(l) {
                tokens = Some(*l);
            }
        } else if let Some(t) = &totals
            && !stale
        {
            let derived = match &previous {
                None => *t,
                Some(p) if !advanced(t, p) => *t,
                Some(p) => delta(t, p),
            };
            if !is_zero(&derived) {
                tokens = Some(derived);
            }
        }
        if let Some(t) = totals {
            previous = Some(t);
        }
        if !is_replay && let Some(t) = tokens {
            match &model {
                None => pre_context_events += 1,
                Some(m) => pending.push(Pending {
                    timestamp,
                    index: line.index,
                    tokens: t,
                    turn: after_user_input,
                    model: m.clone(),
                }),
            }
        }
        after_user_input = false;
    }

    if model.is_none() {
        warnings.push(ReaderWarning {
            message: format!("codex session without turn_context model: {file}\n"),
        });
        skipped.push(file.to_owned());
        return;
    }
    if pre_context_events > 0 {
        warnings.push(ReaderWarning {
            message: format!(
                "skipping {pre_context_events} codex token event(s) before the first turn_context in {file}\n"
            ),
        });
    }
    if pending.is_empty() {
        return;
    }
    for key in file_replay_keys {
        seen_replay_keys.insert(key);
    }
    for item in pending {
        let t = split_reasoning(item.tokens);
        events.push(UsageEvent {
            harness: "codex".to_owned(),
            timestamp: item.timestamp,
            session_id: session_id.clone(),
            message_id: format!("{file}:{}", item.index),
            turn: item.turn,
            subagent: false,
            model: item.model,
            tokens: TokenCounts {
                input: t.input as u64,
                output: t.output as u64,
                cache_read: t.cache_read as u64,
                cache_write: t.cache_write as u64,
                cache_write1h: None,
                reasoning: t.reasoning as u64,
            },
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        });
    }
}

/// Read all codex usage events from the given root. Faithful port of `readCodex`.
pub fn read_codex(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen_replay_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for sub in ["sessions", "archived_sessions"] {
        let dir = root.join(sub);
        if !dir_exists(&dir) {
            continue;
        }
        let files = list_files(&dir, |name| {
            name.starts_with("rollout-") && name.ends_with(".jsonl")
        });
        for file in files {
            read_session_file(
                &file,
                &mut events,
                &mut skipped,
                &mut warnings,
                &mut seen_replay_keys,
            );
        }
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

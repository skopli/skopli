use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, as_string, epoch_to_iso, file_mtime_iso, finite_number,
    is_record, list_files, parent_dir_name, read_json, read_jsonl_lines, reparse_iso,
};
use crate::types::{TokenCounts, UsageEvent};

/// The grok reader wired into the harness registry.
pub struct GrokReader;

impl Reader for GrokReader {
    fn harness_id(&self) -> &'static str {
        "grok"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let home = grok_home(ctx.env("GROK_HOME"), ctx.home());
        read_grok(&home)
    }
}

/// Resolve the grok home. Faithful port of `grokHome`.
pub fn grok_home(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join(".grok")
}

const UNKNOWN_MODEL: &str = "grok-unknown";

/// Faithful port of `getPath`.
fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        if !is_record(current) {
            return None;
        }
        current = current.get(*key)?;
    }
    Some(current)
}

/// Faithful port of `firstString`.
fn first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    for path in paths {
        if let Some(found) = get_path(value, path).and_then(as_string)
            && !found.trim().is_empty()
        {
            return Some(found);
        }
    }
    None
}

/// Faithful port of `timestampFrom`.
fn timestamp_from(value: &Value, fallback: &str) -> String {
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

/// Faithful port of `legacyModel`.
fn legacy_model(value: &Value) -> Option<String> {
    first_string(
        value,
        &[
            &["params", "update", "_meta", "modelId"],
            &["params", "_meta", "modelId"],
            &["params", "modelId"],
            &["model_id"],
            &["modelId"],
            &["model"],
        ],
    )
}

/// Faithful port of `legacyTotal`.
fn legacy_total(value: &Value) -> Option<f64> {
    for path in [
        &["params", "_meta", "totalTokens"][..],
        &["params", "update", "_meta", "totalTokens"][..],
        &["params", "update", "totalTokens"][..],
        &["params", "totalTokens"][..],
        &["usage", "totalTokens"][..],
        &["totalTokens"][..],
    ] {
        if let Some(found) = get_path(value, path).and_then(finite_number) {
            return Some(found);
        }
    }
    None
}

/// Faithful port of `legacyTimestamp`.
fn legacy_timestamp(value: &Value, fallback: &str) -> String {
    for path in [
        &["params", "_meta", "agentTimestampMs"][..],
        &["params", "update", "_meta", "agentTimestampMs"][..],
        &["params", "timestamp"][..],
        &["timestamp"][..],
        &["ts"][..],
    ] {
        if let Some(found) = get_path(value, path) {
            let iso = timestamp_from(found, "");
            if !iso.is_empty() {
                return iso;
            }
        }
    }
    fallback.to_owned()
}

/// Faithful port of `pickAlias`.
fn pick_alias(value: &Value, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(found) = value.get(*key).and_then(finite_number) {
            return found.max(0.0);
        }
    }
    0.0
}

/// Faithful port of `legacyUsageTokens`.
fn legacy_usage_tokens(value: &Value) -> Option<TokenCounts> {
    let usage = get_path(value, &["params", "update", "usage"])?;
    if !is_record(usage) {
        return None;
    }
    let input = pick_alias(usage, &["inputTokens", "input_tokens", "promptTokens"]);
    let output = pick_alias(
        usage,
        &["outputTokens", "output_tokens", "completionTokens"],
    );
    let cache_read = pick_alias(
        usage,
        &[
            "cachedReadTokens",
            "cacheReadTokens",
            "cache_read_input_tokens",
        ],
    );
    let cache_write = pick_alias(
        usage,
        &[
            "cachedWriteTokens",
            "cacheWriteTokens",
            "cacheCreationTokens",
            "cache_creation_input_tokens",
        ],
    );
    let reasoning = pick_alias(
        usage,
        &["reasoningTokens", "thoughtTokens", "thinkingTokens"],
    );
    if input == 0.0 && output == 0.0 && cache_read == 0.0 && cache_write == 0.0 && reasoning == 0.0
    {
        return None;
    }
    let reported_total = usage
        .get("totalTokens")
        .and_then(finite_number)
        .or_else(|| usage.get("total_tokens").and_then(finite_number));
    let inclusive = match reported_total {
        None => true,
        Some(t) => t.max(0.0) == input + output,
    };
    Some(TokenCounts {
        input: if inclusive {
            (input - cache_read).max(0.0) as u64
        } else {
            input as u64
        },
        output: if inclusive {
            (output - reasoning).max(0.0) as u64
        } else {
            output as u64
        },
        cache_read: cache_read as u64,
        cache_write: cache_write as u64,
        cache_write1h: None,
        reasoning: reasoning as u64,
    })
}

fn input_only(amount: u64) -> TokenCounts {
    TokenCounts {
        input: amount,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cache_write1h: None,
        reasoning: 0,
    }
}

/// Faithful port of `signalsEffectiveTotal`.
fn signals_effective_total(value: &Value) -> f64 {
    let before = value
        .get("totalTokensBeforeCompaction")
        .and_then(finite_number)
        .unwrap_or(0.0)
        .max(0.0);
    let total = value
        .get("totalTokens")
        .and_then(finite_number)
        .unwrap_or(0.0)
        .max(0.0);
    let context = value.get("contextTokensUsed");
    match context {
        None => before + total,
        Some(c) => {
            let ctx_val = finite_number(c).unwrap_or(0.0).max(0.0);
            total.max(before + ctx_val)
        }
    }
}

/// Faithful port of `signalsModel`.
fn signals_model(value: &Value) -> Option<String> {
    if let Some(primary) = value.get("primaryModelId").and_then(as_string)
        && !primary.trim().is_empty()
    {
        return Some(primary);
    }
    if let Some(Value::Array(used)) = value.get("modelsUsed")
        && let Some(first) = used.first().and_then(as_string)
        && !first.trim().is_empty()
    {
        return Some(first);
    }
    None
}

struct ActiveTurn {
    baseline_total: f64,
    max_total: f64,
    timestamp: String,
    model: String,
    turn_index: u64,
    turn: bool,
    has_usage: bool,
}

/// Faithful port of `readLegacySession`.
fn read_legacy_session(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let session_id = parent_dir_name(file);
    let fallback_timestamp = file_mtime_iso(file);
    let mut usage_events: Vec<UsageEvent> = Vec::new();
    let mut fallback_events: Vec<UsageEvent> = Vec::new();
    let mut model = UNKNOWN_MODEL.to_owned();
    let mut last_total: Option<f64> = None;
    let mut last_update_timestamp = fallback_timestamp.clone();
    let mut active: Option<ActiveTurn> = None;
    let mut turn_index: u64 = 0;

    let flush = |turn: &ActiveTurn, fallback_events: &mut Vec<UsageEvent>| {
        if turn.has_usage {
            return;
        }
        let delta = turn.max_total - turn.baseline_total;
        if delta <= 0.0 {
            return;
        }
        fallback_events.push(UsageEvent {
            harness: "grok".to_owned(),
            timestamp: turn.timestamp.clone(),
            session_id: session_id.clone(),
            message_id: format!("grok:{}:{}", session_id, turn.turn_index),
            turn: turn.turn,
            subagent: false,
            model: turn.model.clone(),
            tokens: input_only(delta as u64),
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        });
    };

    let lines = read_jsonl_lines(file, skipped, warnings).unwrap_or_default();

    for JsonlLine { index, value } in &lines {
        let record = value;
        if !is_record(record) {
            continue;
        }
        if let Some(seen_model) = legacy_model(record) {
            model = seen_model.clone();
            if let Some(a) = active.as_mut()
                && a.model == UNKNOWN_MODEL
            {
                a.model = model.clone();
            }
        }
        let timestamp = legacy_timestamp(record, &fallback_timestamp);
        if get_path(record, &["params", "update", "sessionUpdate"]).and_then(Value::as_str)
            == Some("user_message_chunk")
        {
            if let Some(a) = active.as_ref() {
                flush(a, &mut fallback_events);
            }
            active = Some(open_turn(
                last_total.unwrap_or(0.0),
                &timestamp,
                true,
                &model,
                &mut turn_index,
            ));
            last_update_timestamp = timestamp.clone();
            continue;
        }
        let usage_tokens = legacy_usage_tokens(record);
        if let Some(tokens) = usage_tokens {
            if active.is_none() {
                active = Some(open_turn(
                    last_total.unwrap_or(0.0),
                    &timestamp,
                    true,
                    &model,
                    &mut turn_index,
                ));
            }
            let a = active.as_mut().unwrap();
            a.has_usage = true;
            usage_events.push(UsageEvent {
                harness: "grok".to_owned(),
                timestamp: timestamp.clone(),
                session_id: session_id.clone(),
                message_id: format!("{file}:{index}"),
                turn: a.turn,
                subagent: false,
                model: model.clone(),
                tokens,
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            });
            a.turn = false;
            last_update_timestamp = timestamp.clone();
        }
        let total = match legacy_total(record) {
            Some(t) if t >= 0.0 => t,
            _ => continue,
        };
        last_update_timestamp = timestamp.clone();
        if let Some(lt) = last_total
            && total < lt
        {
            if let Some(a) = active.as_ref() {
                flush(a, &mut fallback_events);
            }
            last_total = Some(total);
            active = Some(open_turn(total, &timestamp, false, &model, &mut turn_index));
            continue;
        }
        if let Some(lt) = last_total
            && total == lt
        {
            continue;
        }
        if active.is_none() {
            active = Some(open_turn(
                last_total.unwrap_or(0.0),
                &timestamp,
                false,
                &model,
                &mut turn_index,
            ));
        }
        let a = active.as_mut().unwrap();
        if total > a.max_total {
            a.max_total = total;
            a.timestamp = timestamp.clone();
        }
        last_total = Some(total);
    }
    if let Some(a) = active.as_ref() {
        flush(a, &mut fallback_events);
    }
    let counted_input: f64 = usage_events
        .iter()
        .chain(fallback_events.iter())
        .map(|e| (e.tokens.input + e.tokens.cache_read) as f64)
        .sum();
    append_signals_reconciliation(
        file,
        &session_id,
        &mut fallback_events,
        &model,
        counted_input,
        &last_update_timestamp,
    );
    usage_events.into_iter().chain(fallback_events).collect()
}

fn open_turn(
    baseline: f64,
    timestamp: &str,
    marked: bool,
    model: &str,
    turn_index: &mut u64,
) -> ActiveTurn {
    let turn = ActiveTurn {
        baseline_total: baseline,
        max_total: baseline,
        timestamp: timestamp.to_owned(),
        model: model.to_owned(),
        turn_index: *turn_index,
        turn: marked,
        has_usage: false,
    };
    *turn_index += 1;
    turn
}

/// Faithful port of `appendSignalsReconciliation`.
fn append_signals_reconciliation(
    updates_file: &str,
    session_id: &str,
    events: &mut Vec<UsageEvent>,
    fallback_model: &str,
    counted_input: f64,
    timestamp: &str,
) {
    let dir = dirname(updates_file);
    let signals_path = format!("{dir}/signals.json");
    let value = match read_json(&signals_path) {
        Some(v) => v,
        None => return,
    };
    if !is_record(&value) {
        return;
    }
    let signals_total = signals_effective_total(&value);
    if signals_total <= 0.0 {
        return;
    }
    let extra = signals_total - counted_input;
    if extra <= 0.0 {
        return;
    }
    events.push(UsageEvent {
        harness: "grok".to_owned(),
        timestamp: if !timestamp.is_empty() {
            timestamp.to_owned()
        } else {
            file_mtime_iso(updates_file)
        },
        session_id: session_id.to_owned(),
        message_id: format!("grok:{session_id}:signals"),
        turn: false,
        subagent: false,
        model: signals_model(&value).unwrap_or_else(|| fallback_model.to_owned()),
        tokens: input_only(extra as u64),
        calls: None,
        cost_usd: None,
        workspace: None,
        title: None,
    });
}

fn dirname(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rfind('/') {
        Some(i) => normalized[..i].to_owned(),
        None => String::new(),
    }
}

struct UnifiedEntry {
    event: UsageEvent,
    key: String,
}

/// Faithful port of `unifiedIdentity`.
fn unified_identity(record: &Value) -> String {
    let ctx = record.get("ctx").filter(|c| is_record(c));
    for source in [Some(record), ctx].into_iter().flatten() {
        for key in ["event_id", "eventId", "id", "uuid"] {
            if let Some(id) = source.get(key).and_then(as_string)
                && !id.trim().is_empty()
            {
                return format!("id:{id}");
            }
        }
    }
    format!("row:{}", serde_json::to_string(record).unwrap_or_default())
}

/// Faithful port of `readUnifiedLog`.
fn read_unified_log(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UnifiedEntry> {
    let fallback_timestamp = file_mtime_iso(file);
    let mut model_by_session: HashMap<String, String> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<UnifiedEntry> = Vec::new();

    let lines = match read_jsonl_lines(file, skipped, warnings) {
        Some(l) => l,
        None => return out,
    };

    for JsonlLine { index, value } in &lines {
        let record = value;
        if !is_record(record) {
            continue;
        }
        let msg = record.get("msg").and_then(as_string);
        let sid = record.get("sid").and_then(as_string);
        let ctx = record.get("ctx").filter(|c| is_record(c));
        if msg.as_deref() == Some("model changed")
            || msg.as_deref() == Some("model catalog: notifying clients")
        {
            let field = if msg.as_deref() == Some("model changed") {
                "model"
            } else {
                "current_model_id"
            };
            let model = ctx.and_then(|c| c.get(field)).and_then(as_string);
            if let (Some(sid), Some(model)) = (&sid, model)
                && !sid.trim().is_empty()
                && !model.trim().is_empty()
            {
                model_by_session.insert(sid.clone(), model);
            }
            continue;
        }
        if msg.as_deref() != Some("shell.turn.inference_done") {
            continue;
        }
        let sid = match &sid {
            Some(s) if !s.trim().is_empty() => s.clone(),
            _ => continue,
        };
        let ctx = match ctx {
            Some(c) => c,
            None => continue,
        };
        let prompt = ctx.get("prompt_tokens").and_then(finite_number);
        let completion = ctx.get("completion_tokens").and_then(finite_number);
        let (prompt, completion) = match (prompt, completion) {
            (Some(p), Some(c)) if p >= 0.0 && c >= 0.0 => (p, c),
            _ => continue,
        };
        let cached_raw = ctx
            .get("cached_prompt_tokens")
            .and_then(finite_number)
            .unwrap_or(0.0)
            .max(0.0);
        let cached = cached_raw.min(prompt);
        let reasoning_raw = ctx
            .get("reasoning_tokens")
            .and_then(finite_number)
            .unwrap_or(0.0)
            .max(0.0);
        let reasoning = reasoning_raw.min(completion);
        let tokens = TokenCounts {
            input: (prompt - cached) as u64,
            output: (completion - reasoning) as u64,
            cache_read: cached as u64,
            cache_write: 0,
            cache_write1h: None,
            reasoning: reasoning as u64,
        };
        if tokens.input == 0
            && tokens.output == 0
            && tokens.cache_read == 0
            && tokens.reasoning == 0
        {
            continue;
        }
        let loop_index = ctx.get("loop_index").and_then(finite_number).unwrap_or(1.0);
        let timestamp = timestamp_from(
            record.get("ts").unwrap_or(&Value::Null),
            &fallback_timestamp,
        );
        let model = model_by_session
            .get(&sid)
            .cloned()
            .unwrap_or_else(|| UNKNOWN_MODEL.to_owned());
        let key = format!("grok-unified:{sid}:{}", unified_identity(record));
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        out.push(UnifiedEntry {
            event: UsageEvent {
                harness: "grok".to_owned(),
                timestamp,
                session_id: sid,
                message_id: format!("{file}:{index}"),
                turn: loop_index <= 1.0,
                subagent: false,
                model,
                tokens,
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            },
            key,
        });
    }
    out
}

/// Read all grok usage events. Faithful port of `readGrok`.
pub fn read_grok(home: &Path) -> ReaderResult {
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let logs_dir = home.join("logs");
    let unified_files = list_files(&logs_dir, |name| name == "unified.jsonl");
    let mut unified: Vec<UsageEvent> = Vec::new();
    let mut seen_unified: HashSet<String> = HashSet::new();
    for file in &unified_files {
        for entry in read_unified_log(file, &mut skipped, &mut warnings) {
            if seen_unified.contains(&entry.key) {
                continue;
            }
            seen_unified.insert(entry.key.clone());
            unified.push(entry.event);
        }
    }
    let unified_sessions: HashSet<String> = unified.iter().map(|e| e.session_id.clone()).collect();
    let sessions_dir = home.join("sessions");
    let legacy_files = list_files(&sessions_dir, |name| name == "updates.jsonl");
    let mut events = unified;
    for file in &legacy_files {
        for event in read_legacy_session(file, &mut skipped, &mut warnings) {
            if unified_sessions.contains(&event.session_id) {
                continue;
            }
            events.push(event);
        }
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

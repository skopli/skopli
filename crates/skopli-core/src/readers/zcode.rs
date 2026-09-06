use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, file_stem_name, finite_number, is_record, list_files,
    parse_timestamp_to_iso, read_jsonl_lines,
};
use super::sqlite_store::{open_readonly, query_rows};
use crate::types::{TokenCounts, UsageEvent};

/// The zcode reader wired into the harness registry.
///
/// zcode has two lanes: an authoritative SQLite store (`cli/db/db.sqlite`) and a
/// legacy JSONL store (`projects/*.jsonl`). The SQLite lane (rusqlite) runs
/// first and is authoritative, feeding every session it
/// emits into `covered_sessions` so a mid-migration JSONL mirror of that
/// session is suppressed wholesale.
pub struct ZcodeReader;

impl Reader for ZcodeReader {
    fn harness_id(&self) -> &'static str {
        "zcode"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = zcode_root(ctx.env("ZCODE_DATA_DIR"), ctx.home());
        read_zcode(&root)
    }
}

const ZCODE_PROVIDER: &str = "zhipu";
const ZCODE_DEFAULT_MODEL: &str = "glm-5.2";

/// Resolve the zcode data root. Faithful port of `zcodeRoot`.
pub fn zcode_root(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join(".zcode")
}

/// The JSONL projects root under a resolved zcode root. Port of
/// `zcodeProjectsRoot`.
pub fn zcode_projects_root(root: &Path) -> PathBuf {
    root.join("projects")
}

/// The SQLite store path under a resolved zcode root. Port of `zcodeDbPath`.
pub fn zcode_db_path(root: &Path) -> PathBuf {
    root.join("cli").join("db").join("db.sqlite")
}

/// Faithful port of `firstNumber`: the first finite key wins, clamped `>= 0`.
fn first_number(record: &Value, keys: &[&str]) -> u64 {
    for key in keys {
        if let Some(value) = record.get(*key).and_then(finite_number) {
            return value.max(0.0) as u64;
        }
    }
    0
}

/// zcode reports input inclusive of cache and output inclusive of reasoning;
/// strip the overlap so buckets never double-count. Faithful port of
/// `normalize`.
fn normalize(tokens: &TokenCounts) -> TokenCounts {
    TokenCounts {
        input: tokens
            .input
            .saturating_sub(tokens.cache_read)
            .saturating_sub(tokens.cache_write),
        output: tokens.output.saturating_sub(tokens.reasoning),
        cache_read: tokens.cache_read,
        cache_write: tokens.cache_write,
        cache_write1h: None,
        reasoning: tokens.reasoning,
    }
}

fn all_zero(tokens: &TokenCounts) -> bool {
    tokens.input == 0
        && tokens.output == 0
        && tokens.cache_read == 0
        && tokens.cache_write == 0
        && tokens.reasoning == 0
}

/// Read the legacy JSONL store. Faithful port of `readJsonlStore`.
fn read_jsonl_store(
    root: &Path,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
    seen: &mut HashSet<String>,
    covered_sessions: &HashSet<String>,
) -> Vec<UsageEvent> {
    let mut events: Vec<UsageEvent> = Vec::new();
    let files = list_files(&zcode_projects_root(root), |name| name.ends_with(".jsonl"));
    for file in &files {
        let lines = match read_jsonl_lines(file, skipped, warnings) {
            Some(l) => l,
            None => continue,
        };
        let session_from_name = file_stem_name(file);
        let mut current_model = ZCODE_DEFAULT_MODEL.to_owned();
        let mut pending_turn = false;
        let mut assistant_index: usize = 0;
        for line in &lines {
            if !is_record(&line.value) {
                continue;
            }
            let entry = &line.value;
            let role = entry.get("role").and_then(as_string);
            if let Some(model) = entry.get("model").and_then(as_string)
                && !model.trim().is_empty()
            {
                current_model = model.trim().to_lowercase();
            }
            if role.as_deref() == Some("user") {
                pending_turn = true;
                continue;
            }
            if role.as_deref() != Some("assistant") {
                continue;
            }
            let usage = entry
                .get("usage")
                .filter(|u| is_record(u))
                .or_else(|| entry.get("token_usage").filter(|u| is_record(u)));
            let index = assistant_index;
            assistant_index += 1;
            // a dropped row must not consume the pending human turn
            let usage = match usage {
                Some(u) => u,
                None => continue,
            };
            let raw = TokenCounts {
                input: first_number(
                    usage,
                    &["input", "input_tokens", "prompt_tokens", "inputTokens"],
                ),
                output: first_number(
                    usage,
                    &[
                        "output",
                        "output_tokens",
                        "completion_tokens",
                        "outputTokens",
                    ],
                ),
                cache_read: first_number(
                    usage,
                    &["input_cache_read", "cache_read_tokens", "cacheReadTokens"],
                ),
                cache_write: first_number(
                    usage,
                    &[
                        "input_cache_creation",
                        "cache_write_tokens",
                        "cacheCreationTokens",
                    ],
                ),
                cache_write1h: None,
                reasoning: first_number(usage, &["reasoning", "reasoningTokens"]),
            };
            let tokens = normalize(&raw);
            if all_zero(&tokens) {
                continue;
            }
            let session_id = entry
                .get("sessionId")
                .and_then(as_string)
                .unwrap_or_else(|| session_from_name.clone());
            // the SQLite store is authoritative; a session already emitted from it
            // is a migration duplicate here and is suppressed wholesale
            if covered_sessions.contains(&session_id) {
                continue;
            }
            let timestamp =
                match parse_timestamp_to_iso(entry.get("timestamp").unwrap_or(&Value::Null)) {
                    Some(t) => t,
                    None => {
                        let loc = format!("{file}:{}", line.index + 1);
                        skipped.push(loc.clone());
                        warnings.push(ReaderWarning {
                            message: format!("skipping malformed zcode usage {loc}\n"),
                        });
                        continue;
                    }
                };
            let message_id = format!("zcode:{session_id}:{index}");
            if seen.contains(&message_id) {
                continue;
            }
            seen.insert(message_id.clone());
            let turn = pending_turn;
            pending_turn = false;
            events.push(UsageEvent {
                harness: "zcode".to_owned(),
                timestamp,
                session_id,
                message_id,
                turn,
                subagent: false,
                model: format!("{ZCODE_PROVIDER}/{current_model}"),
                tokens,
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            });
        }
    }
    events
}

/// Read the authoritative SQLite store. Faithful port of `readDbStore`: one
/// event per non-zero `model_usage` row, with the earliest-started row per
/// `turn_id` flagged as a turn. Every emitted session id feeds
/// `covered_sessions` so its JSONL mirror is suppressed.
fn read_db_store(
    db_path: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
    seen: &mut HashSet<String>,
    covered_sessions: &mut HashSet<String>,
) -> Vec<UsageEvent> {
    let conn = match open_readonly(db_path) {
        Ok(c) => c,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read zcode database {db_path}: {e}\n"),
            });
            skipped.push(db_path.to_owned());
            return Vec::new();
        }
    };
    let rows = match query_rows(
        &conn,
        "SELECT id, session_id, turn_id, model_id, started_at, completed_at, duration_ms, \
         input_tokens, output_tokens, reasoning_tokens, cache_read_input_tokens, \
         cache_creation_input_tokens FROM model_usage",
    ) {
        Ok(r) => r,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read zcode database {db_path}: {e}\n"),
            });
            skipped.push(db_path.to_owned());
            return Vec::new();
        }
    };

    struct Parsed {
        turn_id: Option<String>,
        started: Option<f64>,
        event: UsageEvent,
    }

    // only the earliest-started row per turn_id starts a turn
    let mut earliest_by_turn: HashMap<String, f64> = HashMap::new();
    let mut parsed: Vec<Parsed> = Vec::new();

    for row in &rows {
        if !is_record(row) {
            continue;
        }
        let id = row
            .get("id")
            .and_then(as_string)
            .unwrap_or_else(|| match row.get("id") {
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            });
        if id.is_empty() {
            continue;
        }
        let session_id = row
            .get("session_id")
            .and_then(as_string)
            .unwrap_or_else(|| "unknown".to_owned());
        let model = row.get("model_id").and_then(as_string);
        let started_ms = row.get("started_at").and_then(finite_number);
        let completed_ms = row.get("completed_at").and_then(finite_number);
        let duration_ms = row.get("duration_ms").and_then(finite_number);
        let started = match started_ms {
            Some(s) => Some(s),
            None => match (completed_ms, duration_ms) {
                (Some(c), Some(d)) => Some(c - d),
                (c, _) => c,
            },
        };
        let num = |k: &str| -> u64 {
            row.get(k)
                .and_then(finite_number)
                .map(|n| n.max(0.0) as u64)
                .unwrap_or(0)
        };
        let raw = TokenCounts {
            input: num("input_tokens"),
            output: num("output_tokens"),
            cache_read: num("cache_read_input_tokens"),
            cache_write: num("cache_creation_input_tokens"),
            cache_write1h: None,
            reasoning: num("reasoning_tokens"),
        };
        let tokens = normalize(&raw);
        if all_zero(&tokens) {
            continue;
        }
        let message_id = format!("zcode-sqlite:{id}");
        if seen.contains(&message_id) {
            continue;
        }
        let timestamp = match started {
            Some(s) if s > 0.0 => parse_timestamp_to_iso(&Value::from(s)),
            _ => None,
        };
        let timestamp = match timestamp {
            Some(t) => t,
            None => {
                skipped.push(format!("{db_path}:{id}"));
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed zcode usage {db_path}:{id}\n"),
                });
                continue;
            }
        };
        seen.insert(message_id.clone());
        covered_sessions.insert(session_id.clone());
        let turn_id = row.get("turn_id").and_then(as_string);
        if let (Some(tid), Some(s)) = (&turn_id, started) {
            let prior = earliest_by_turn.get(tid).copied();
            if prior.is_none_or(|p| s < p) {
                earliest_by_turn.insert(tid.clone(), s);
            }
        }
        let model_lower = model
            .unwrap_or_else(|| ZCODE_DEFAULT_MODEL.to_owned())
            .to_lowercase();
        parsed.push(Parsed {
            turn_id,
            started,
            event: UsageEvent {
                harness: "zcode".to_owned(),
                timestamp,
                session_id,
                message_id,
                turn: false,
                subagent: false,
                model: format!("{ZCODE_PROVIDER}/{model_lower}"),
                tokens,
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            },
        });
    }

    for item in &mut parsed {
        if let (Some(tid), Some(s)) = (&item.turn_id, item.started) {
            item.event.turn = earliest_by_turn.get(tid).copied() == Some(s);
        }
    }
    parsed.into_iter().map(|item| item.event).collect()
}

/// Read all zcode usage events: the authoritative SQLite store first (feeding
/// `covered_sessions`), then the legacy JSONL store. Faithful port of
/// `readZcode`.
pub fn read_zcode(root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut covered_sessions: HashSet<String> = HashSet::new();

    let db_path = zcode_db_path(root);
    if db_path.exists() {
        let db_str = db_path.to_string_lossy().into_owned();
        events.extend(read_db_store(
            &db_str,
            &mut skipped,
            &mut warnings,
            &mut seen,
            &mut covered_sessions,
        ));
    }
    events.extend(read_jsonl_store(
        root,
        &mut skipped,
        &mut warnings,
        &mut seen,
        &covered_sessions,
    ));
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

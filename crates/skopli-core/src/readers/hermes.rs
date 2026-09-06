use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, epoch_to_iso, finite_number, parent_dir_name,
};
use super::sqlite_store::{open_readonly, query_rows, table_exists};
use crate::types::{TokenCounts, UsageEvent};

/// The hermes reader wired into the harness registry. It reads one or more
/// `state.db` files across the resolved homes (root + per-profile), preferring
/// per-(session,model) rows and falling back to session totals. Faithful port
/// of `readHermes`.
pub struct HermesReader;

impl Reader for HermesReader {
    fn harness_id(&self) -> &'static str {
        "hermes"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let homes = hermes_homes(ctx.env("HERMES_HOME"), ctx.home());
        let paths = hermes_db_paths(&homes);
        read_hermes(&paths)
    }
}

/// Resolve the hermes home directories. Faithful port of `hermesHomes`.
pub fn hermes_homes(override_val: Option<&str>, home: &Path) -> Vec<PathBuf> {
    if let Some(v) = override_val
        && !v.is_empty()
    {
        return v
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    vec![home.join(".hermes")]
}

/// Derive the `state.db` paths from resolved homes. Faithful port of
/// `hermesDbPaths`: a home already inside `profiles/` isolates to that profile;
/// otherwise the home's own `state.db` plus every `profiles/*/state.db`. Order
/// is preserved and duplicates removed (first occurrence wins).
pub fn hermes_db_paths(homes: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for home in homes {
        let home_str = home.to_string_lossy().into_owned();
        if parent_dir_name(&home_str) == "profiles" {
            paths.push(home.join("state.db"));
            continue;
        }
        paths.push(home.join("state.db"));
        let profiles_dir = home.join("profiles");
        if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
            let mut children: Vec<PathBuf> = Vec::new();
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    children.push(entry.path().join("state.db"));
                }
            }
            children.sort();
            paths.extend(children);
        }
    }
    // dedup preserving first-occurrence order (mirrors `[...new Set(paths)]`).
    let mut seen: HashSet<PathBuf> = HashSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

const PER_MODEL_QUERY: &str = "SELECT smu.session_id, smu.model, smu.billing_provider, s.started_at, \
COALESCE(s.message_count, 0) AS message_count, \
SUM(smu.input_tokens) AS input_tokens, SUM(smu.output_tokens) AS output_tokens, \
SUM(smu.cache_read_tokens) AS cache_read_tokens, \
SUM(smu.cache_write_tokens) AS cache_write_tokens, \
SUM(smu.reasoning_tokens) AS reasoning_tokens, \
SUM(COALESCE(NULLIF(smu.actual_cost_usd, 0), smu.estimated_cost_usd, 0)) AS cost_usd \
FROM session_model_usage smu JOIN sessions s ON s.id = smu.session_id \
WHERE smu.model IS NOT NULL AND TRIM(smu.model) != '' \
GROUP BY smu.session_id, smu.model, smu.billing_provider, s.started_at, s.message_count \
HAVING SUM(smu.input_tokens) > 0 OR SUM(smu.output_tokens) > 0 \
OR SUM(smu.cache_read_tokens) > 0 OR SUM(smu.cache_write_tokens) > 0 \
OR SUM(smu.reasoning_tokens) > 0 \
OR SUM(COALESCE(NULLIF(smu.actual_cost_usd, 0), smu.estimated_cost_usd, 0)) > 0 \
ORDER BY smu.session_id, \
CASE WHEN smu.model = s.model THEN 0 ELSE 1 END, smu.model, smu.billing_provider";

const SESSION_TOTALS_QUERY: &str = "SELECT id, model, billing_provider, started_at, COALESCE(message_count, 0) AS message_count, \
input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens \
FROM sessions WHERE model IS NOT NULL AND TRIM(model) != ''";

// angle brackets keep this out of the slug space real provider ids occupy
const NULL_PROVIDER_KEY: &str = "<null>";

#[derive(Clone)]
struct HermesRow {
    session_id: String,
    model: String,
    provider: Option<String>,
    started_at: Value,
    message_count: u64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    reasoning: u64,
}

/// Decode a query row into a `HermesRow`, or `None`. Faithful port of
/// `decodeRow`.
fn decode_row(fields: &Value) -> Option<HermesRow> {
    let session_id = fields
        .get("session_id")
        .and_then(as_string)
        .or_else(|| fields.get("id").and_then(as_string))
        .unwrap_or_default();
    let model = fields
        .get("model")
        .and_then(as_string)
        .map(|m| m.trim().to_owned())
        .unwrap_or_default();
    if session_id.is_empty() || model.is_empty() {
        return None;
    }
    let num = |k: &str| -> u64 {
        fields
            .get(k)
            .and_then(finite_number)
            .map(|n| n.max(0.0) as u64)
            .unwrap_or(0)
    };
    Some(HermesRow {
        session_id,
        model,
        provider: fields.get("billing_provider").and_then(as_string),
        started_at: fields.get("started_at").cloned().unwrap_or(Value::Null),
        message_count: num("message_count"),
        input: num("input_tokens"),
        output: num("output_tokens"),
        cache_read: num("cache_read_tokens"),
        cache_write: num("cache_write_tokens"),
        reasoning: num("reasoning_tokens"),
    })
}

fn has_tokens(row: &HermesRow) -> bool {
    row.input > 0
        || row.output > 0
        || row.cache_read > 0
        || row.cache_write > 0
        || row.reasoning > 0
}

/// Build a `UsageEvent` from a row. Faithful port of `buildEvent`.
fn build_event(
    row: &HermesRow,
    db_path: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Option<UsageEvent> {
    let timestamp = match epoch_to_iso(&row.started_at) {
        Some(t) => t,
        None => {
            warnings.push(ReaderWarning {
                message: format!(
                    "skipping malformed hermes session {db_path}:{}\n",
                    row.session_id
                ),
            });
            skipped.push(format!("{db_path}:{}", row.session_id));
            return None;
        }
    };
    let model = match &row.provider {
        Some(p) if !p.is_empty() => format!("{p}/{}", row.model),
        _ => row.model.clone(),
    };
    Some(UsageEvent {
        harness: "hermes".to_owned(),
        timestamp,
        session_id: row.session_id.clone(),
        message_id: row.session_id.clone(),
        turn: false,
        subagent: false,
        model,
        tokens: TokenCounts {
            input: row.input,
            output: row.output,
            cache_read: row.cache_read,
            cache_write: row.cache_write,
            cache_write1h: None,
            reasoning: row.reasoning,
        },
        calls: Some(row.message_count),
        cost_usd: None,
        workspace: None,
        title: None,
    })
}

/// Read all hermes usage events. Faithful port of `readHermes`.
pub fn read_hermes(db_paths: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let paths: Vec<(String, PathBuf)> = db_paths
        .iter()
        .filter(|p| p.exists())
        .map(|p| (p.to_string_lossy().into_owned(), p.clone()))
        .collect();

    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut covered_sessions: HashSet<String> = HashSet::new();
    let mut counted_sessions: HashSet<String> = HashSet::new();

    // authoritative per-session call total = reader-wide max message_count.
    let mut session_count: HashMap<String, u64> = HashMap::new();
    let note_count = |session_id: &str, count: u64, session_count: &mut HashMap<String, u64>| {
        let prior = session_count.get(session_id).copied().unwrap_or(0);
        if count > prior {
            session_count.insert(session_id.to_owned(), count);
        }
    };

    // pre-pass: reader-wide max message_count from every db's session totals.
    for (db_str, _) in &paths {
        if let Some(conn) = open_db(db_str, &mut skipped, &mut warnings)
            && let Ok(rows) = query_rows(&conn, SESSION_TOTALS_QUERY)
        {
            for raw in &rows {
                if let Some(row) = decode_row(raw) {
                    note_count(&row.session_id, row.message_count, &mut session_count);
                }
            }
        }
    }

    // pass 1: per-model rows first, across every db.
    for (db_str, _) in &paths {
        let rows = match open_db(db_str, &mut skipped, &mut warnings) {
            Some(conn) => {
                if !table_exists(&conn, "session_model_usage") {
                    Vec::new()
                } else {
                    match query_rows(&conn, PER_MODEL_QUERY) {
                        Ok(raws) => raws.iter().filter_map(decode_row).collect(),
                        Err(e) => {
                            warnings.push(ReaderWarning {
                                message: format!("hermes per-model query failed {db_str}: {e}\n"),
                            });
                            Vec::new()
                        }
                    }
                }
            }
            None => continue,
        };
        for mut row in rows {
            let provider_key = row
                .provider
                .clone()
                .unwrap_or_else(|| NULL_PROVIDER_KEY.to_owned());
            let key = format!("hermes:{}:{}:{}", row.session_id, row.model, provider_key);
            if seen_keys.contains(&key) {
                continue;
            }
            let authoritative = session_count.get(&row.session_id).copied().unwrap_or(0);
            let claims_count = !counted_sessions.contains(&row.session_id) && authoritative > 0;
            row.message_count = if claims_count { authoritative } else { 0 };
            let event = match build_event(&row, db_str, &mut skipped, &mut warnings) {
                Some(e) => e,
                None => continue,
            };
            covered_sessions.insert(row.session_id.clone());
            if claims_count {
                counted_sessions.insert(row.session_id.clone());
            }
            seen_keys.insert(key);
            events.push(event);
        }
    }

    // pass 2: session totals for sessions no per-model pass covered.
    for (db_str, _) in &paths {
        let rows: Vec<HermesRow> = match open_db(db_str, &mut skipped, &mut warnings) {
            Some(conn) => match query_rows(&conn, SESSION_TOTALS_QUERY) {
                Ok(raws) => raws
                    .iter()
                    .filter_map(decode_row)
                    .filter(has_tokens)
                    .collect(),
                Err(_) => Vec::new(),
            },
            None => continue,
        };
        for mut row in rows {
            if covered_sessions.contains(&row.session_id) {
                continue;
            }
            if seen_keys.contains(&row.session_id) {
                continue;
            }
            row.message_count = session_count
                .get(&row.session_id)
                .copied()
                .unwrap_or(row.message_count);
            let event = match build_event(&row, db_str, &mut skipped, &mut warnings) {
                Some(e) => e,
                None => continue,
            };
            seen_keys.insert(row.session_id.clone());
            events.push(event);
        }
    }

    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

/// Open a hermes database read-only, warning + recording a skip on failure.
/// Mirrors the TS `withDatabase` open-failure path.
fn open_db(
    db_path: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Option<rusqlite::Connection> {
    match open_readonly(db_path) {
        Ok(c) => Some(c),
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read hermes database {db_path}: {e}\n"),
            });
            skipped.push(db_path.to_owned());
            None
        }
    }
}

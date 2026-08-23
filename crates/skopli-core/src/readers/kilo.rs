use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, file_mtime_iso, finite_number, is_record,
};
use super::sqlite_store::{
    ParsedValue, SqliteMessageRow, SqliteMessageStoreOptions, read_sqlite_message_store,
};
use crate::time::to_iso_string;
use crate::types::{TokenCounts, UsageEvent};

/// The kilo reader wired into the harness registry. It reads the kilo SQLite
/// `message` store, one row per message, and reconstructs per-session turn
/// ordering. Faithful port of `readKilo` (src/readers/kilo.ts).
pub struct KiloReader;

impl Reader for KiloReader {
    fn harness_id(&self) -> &'static str {
        "kilo"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let path = kilo_db_path(ctx.env("KILO_DATA_DIR"), ctx.home());
        read_kilo(&path)
    }
}

/// Resolve the kilo database path. Faithful port of `kiloDbPath`:
/// `KILO_DATA_DIR` overrides, else `~/.local/share/kilo/kilo.db`.
pub fn kilo_db_path(override_val: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_val
        && !v.is_empty()
    {
        return PathBuf::from(v).join("kilo.db");
    }
    home.join(".local")
        .join("share")
        .join("kilo")
        .join("kilo.db")
}

/// One decoded kilo message, mirroring the TS `KiloMessage`.
struct KiloMessage {
    id: String,
    session_id: String,
    role: String,
    created: f64,
    event: Option<UsageEvent>,
}

/// Parse one kilo `message` row + its JSON `data`. Faithful port of
/// `parseKiloMessage`.
fn parse_kilo_message(
    row: &SqliteMessageRow,
    raw: &Value,
    db_path: &str,
) -> ParsedValue<KiloMessage> {
    if !is_record(raw) {
        return ParsedValue {
            value: None,
            malformed: true,
        };
    }
    let id = as_string(&raw["id"]).or_else(|| row.get("id").and_then(as_string));
    let session_id =
        as_string(&raw["session_id"]).or_else(|| row.get("session_id").and_then(as_string));
    let role = as_string(&raw["role"]);
    // created = data.time.created, scaled to millis (seconds -> * 1000 when < 1e12)
    let raw_created = raw
        .get("time")
        .filter(|t| is_record(t))
        .and_then(|t| finite_number(&t["created"]));
    let created = match raw_created {
        Some(c) if c > 0.0 => c * (if c < 1e12 { 1000.0 } else { 1.0 }),
        _ => 0.0,
    };
    let (id, session_id, role) = match (id, session_id, role) {
        (Some(i), Some(s), Some(r)) => (i, s, r),
        _ => {
            return ParsedValue {
                value: None,
                malformed: true,
            };
        }
    };
    if role != "assistant" {
        return ParsedValue {
            value: Some(KiloMessage {
                id,
                session_id,
                role,
                created,
                event: None,
            }),
            malformed: false,
        };
    }
    let model = as_string(&raw["modelID"]);
    let tokens_val = raw.get("tokens");
    let (model, tokens_val) = match (model, tokens_val) {
        (Some(m), Some(t)) if is_record(t) => (m, t),
        _ => {
            return ParsedValue {
                value: None,
                malformed: true,
            };
        }
    };
    let cache = tokens_val.get("cache");
    let input = finite_number(&tokens_val["input"]);
    let output = finite_number(&tokens_val["output"]);
    let cache_read = cache
        .filter(|c| is_record(c))
        .and_then(|c| finite_number(&c["read"]));
    let cache_write = cache
        .filter(|c| is_record(c))
        .and_then(|c| finite_number(&c["write"]));
    let (input, output, cache_read, cache_write) = match (input, output, cache_read, cache_write) {
        (Some(i), Some(o), Some(r), Some(w)) => (i, o, r, w),
        _ => {
            return ParsedValue {
                value: None,
                malformed: true,
            };
        }
    };
    let reasoning = finite_number(&tokens_val["reasoning"])
        .unwrap_or(0.0)
        .max(0.0);
    let timestamp = if created == 0.0 {
        file_mtime_iso(db_path)
    } else {
        // `new Date(created).toISOString()`; created is finite and > 0 here.
        to_iso_string(created.trunc() as i64).unwrap_or_else(|| file_mtime_iso(db_path))
    };
    let event = UsageEvent {
        harness: "kilo".to_owned(),
        timestamp,
        session_id: session_id.clone(),
        message_id: id.clone(),
        turn: false,
        subagent: false,
        model,
        tokens: TokenCounts {
            input: input.max(0.0) as u64,
            output: output.max(0.0) as u64,
            cache_read: cache_read.max(0.0) as u64,
            cache_write: cache_write.max(0.0) as u64,
            cache_write1h: None,
            reasoning: reasoning as u64,
        },
        calls: None,
        cost_usd: None,
        workspace: None,
        title: None,
    };
    ParsedValue {
        value: Some(KiloMessage {
            id,
            session_id,
            role,
            created,
            event: Some(event),
        }),
        malformed: false,
    }
}

/// Read all kilo usage events. Faithful port of `readKilo`.
pub fn read_kilo(db_path: &Path) -> ReaderResult {
    if !db_path.exists() {
        return ReaderResult::default();
    }
    let db_str = db_path.to_string_lossy().into_owned();
    let mut parse = |row: &SqliteMessageRow, raw: &Value| parse_kilo_message(row, raw, &db_str);
    let (values, mut result) = match read_sqlite_message_store(SqliteMessageStoreOptions {
        db_path: &db_str,
        query: "SELECT id, session_id, 0 AS time_created, data FROM message",
        parse: &mut parse,
        malformed_label: "kilo message",
    }) {
        Ok(v) => v,
        Err(e) => {
            return ReaderResult {
                events: Vec::new(),
                skipped: vec![db_str.clone()],
                warnings: vec![ReaderWarning {
                    message: format!("unable to read kilo database {db_str}: {e}\n"),
                }],
            };
        }
    };

    // Group by session preserving first-seen session order, then per-session
    // sort by (created, id) and reconstruct turns.
    let mut by_session: HashMap<String, Vec<KiloMessage>> = HashMap::new();
    let mut session_order: Vec<String> = Vec::new();
    for value in values {
        if !by_session.contains_key(&value.session_id) {
            session_order.push(value.session_id.clone());
        }
        by_session
            .entry(value.session_id.clone())
            .or_default()
            .push(value);
    }

    let mut seen: HashSet<String> = HashSet::new();
    for session_id in &session_order {
        let messages = by_session.get_mut(session_id).expect("session present");
        messages.sort_by(|a, b| {
            a.created
                .partial_cmp(&b.created)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut previous_was_user = false;
        for message in messages.iter_mut() {
            if let Some(event) = message.event.as_mut()
                && !seen.contains(&event.message_id)
            {
                seen.insert(event.message_id.clone());
                event.turn = previous_was_user;
                result.events.push(event.clone());
            }
            if message.role == "assistant" {
                previous_was_user = false;
            } else if message.role == "user" {
                previous_was_user = true;
            }
        }
    }
    result
}

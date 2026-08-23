use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, finite_number, is_record, list_files,
    parse_timestamp_to_iso, read_jsonl_lines,
};
use super::sqlite_store::{open_readonly, query_rows};
use crate::types::{TokenCounts, UsageEvent};

const DEVIN_PROVIDER: &str = "devin";

/// The devin reader wired into the harness registry. It reads the CLI SQLite
/// store (authoritative) and the Desktop ACP `.ndjson` store, dropping Desktop
/// usage for any session the CLI already covered. Faithful port of `readDevin`.
pub struct DevinReader;

impl Reader for DevinReader {
    fn harness_id(&self) -> &'static str {
        "devin"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let db_path = devin_db_path(ctx.env("DEVIN_DATA_DIR"), ctx.home());
        let desktop_root = devin_desktop_root(ctx.env("DEVIN_DESKTOP_DIR"), ctx.home());
        read_devin(&db_path, &desktop_root)
    }
}

/// Resolve the CLI database path. Faithful port of `devinDbPath`.
pub fn devin_db_path(data_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = data_dir
        && !v.is_empty()
    {
        return PathBuf::from(v).join("cli").join("sessions.db");
    }
    home.join(".local")
        .join("share")
        .join("devin")
        .join("cli")
        .join("sessions.db")
}

/// Resolve the Desktop ACP-events root. Faithful port of `devinDesktopRoot`.
pub fn devin_desktop_root(desktop_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = desktop_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join("Library")
        .join("Application Support")
        .join("Devin")
        .join("User")
        .join("acp-events")
}

/// "adaptive" is a routing mode, not a model. Faithful port of `resolveModel`.
fn resolve_model(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "adaptive" {
        return None;
    }
    Some(trimmed.to_owned())
}

/// Follow a key path through nested records. Faithful port of `readAtPointer`.
fn read_at_pointer<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = root;
    for key in path {
        if !is_record(current) {
            return None;
        }
        current = current.get(*key)?;
    }
    Some(current)
}

struct CliSession {
    title: Option<String>,
}

struct CliRead {
    events: Vec<UsageEvent>,
    title_to_session: HashMap<String, Option<String>>,
}

/// The `String(value ?? "")` fallback the TS uses for a non-string row id.
fn row_id_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Read the CLI SQLite store. Faithful port of `readCli`.
fn read_cli(
    db_path: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> CliRead {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut sessions: HashMap<String, CliSession> = HashMap::new();

    let conn = match open_readonly(db_path) {
        Ok(c) => c,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read devin database {db_path}: {e}\n"),
            });
            skipped.push(db_path.to_owned());
            return CliRead {
                events,
                title_to_session: HashMap::new(),
            };
        }
    };

    match query_rows(
        &conn,
        "SELECT id, title, model, working_directory FROM sessions",
    ) {
        Ok(rows) => {
            for row in &rows {
                if !is_record(row) {
                    continue;
                }
                let id = match row.get("id").and_then(as_string) {
                    Some(id) => id,
                    None => continue,
                };
                sessions.insert(
                    id,
                    CliSession {
                        title: row.get("title").and_then(as_string),
                    },
                );
            }
        }
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read devin database {db_path}: {e}\n"),
            });
            skipped.push(db_path.to_owned());
            return CliRead {
                events,
                title_to_session: HashMap::new(),
            };
        }
    }

    let rows = match query_rows(
        &conn,
        "SELECT m.row_id AS row_id, m.session_id AS session_id, m.chat_message AS chat_message, \
         m.created_at AS created_at, s.model AS session_model \
         FROM message_nodes m JOIN sessions s ON s.id = m.session_id ORDER BY m.row_id",
    ) {
        Ok(r) => r,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read devin database {db_path}: {e}\n"),
            });
            skipped.push(db_path.to_owned());
            return CliRead {
                events,
                title_to_session: HashMap::new(),
            };
        }
    };

    for row in &rows {
        if !is_record(row) {
            continue;
        }
        let row_id = row
            .get("row_id")
            .and_then(as_string)
            .unwrap_or_else(|| row_id_string(row.get("row_id")));
        let session_id = row.get("session_id").and_then(as_string);
        let chat_raw = row.get("chat_message").and_then(as_string);
        let (session_id, chat_raw) = match (session_id, chat_raw) {
            (Some(s), Some(c)) => (s, c),
            _ => continue,
        };
        let chat: Value = match serde_json::from_str(&chat_raw) {
            Ok(v) => v,
            Err(_) => {
                skipped.push(format!("{db_path}:{row_id}"));
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed devin message {db_path}:{row_id}\n"),
                });
                continue;
            }
        };
        if !is_record(&chat) || chat.get("role").and_then(as_string).as_deref() != Some("assistant")
        {
            continue;
        }
        let metadata = match chat.get("metadata") {
            Some(m) if is_record(m) => m,
            _ => continue,
        };
        let metrics = metadata.get("metrics").filter(|m| is_record(m));
        let tokens = if let Some(metrics) = metrics {
            TokenCounts {
                input: max0(metrics.get("input_tokens")),
                output: max0(metrics.get("output_tokens")),
                cache_read: max0(metrics.get("cache_read_tokens")),
                cache_write: max0(metrics.get("cache_creation_tokens")),
                cache_write1h: None,
                reasoning: 0,
            }
        } else {
            let num_tokens = metadata.get("num_tokens").and_then(finite_number);
            match num_tokens {
                Some(n) if n > 0.0 => TokenCounts {
                    input: 0,
                    output: n as u64,
                    cache_read: 0,
                    cache_write: 0,
                    cache_write1h: None,
                    reasoning: 0,
                },
                _ => continue,
            }
        };
        if tokens.input == 0
            && tokens.output == 0
            && tokens.cache_read == 0
            && tokens.cache_write == 0
        {
            // a zero-usage row must not become a precedence marker
            continue;
        }
        let model = resolve_model(metadata.get("generation_model").and_then(as_string))
            .or_else(|| resolve_model(row.get("session_model").and_then(as_string)));
        let model = match model {
            Some(m) => m,
            None => continue,
        };
        let created_seconds = row.get("created_at").and_then(finite_number);
        let total_time_ms = read_at_pointer(metadata, &["total_time_ms"]).and_then(finite_number);
        let created_ms = created_seconds.map(|s| s * 1000.0);
        let start_ms = match (created_ms, total_time_ms) {
            (Some(c), Some(t)) => Some(c - t),
            (c, _) => c,
        };
        let timestamp = match start_ms {
            Some(s) if s > 0.0 => parse_timestamp_to_iso(&Value::from(s)),
            _ => None,
        };
        let timestamp = match timestamp {
            Some(t) => t,
            None => {
                skipped.push(format!("{db_path}:{row_id}"));
                warnings.push(ReaderWarning {
                    message: format!(
                        "skipping devin message with unusable timestamp {db_path}:{row_id}\n"
                    ),
                });
                continue;
            }
        };
        events.push(UsageEvent {
            harness: "devin".to_owned(),
            timestamp,
            session_id: session_id.clone(),
            message_id: format!("devin-cli:{session_id}:{row_id}"),
            turn: true,
            subagent: false,
            model: format!("{DEVIN_PROVIDER}/{model}"),
            tokens,
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        });
    }

    // build a title -> session lookup; an ambiguous title (>1 session) is left
    // unresolved (None).
    let mut title_counts: HashMap<String, HashSet<String>> = HashMap::new();
    for (id, session) in &sessions {
        let title = match &session.title {
            Some(t) if !t.trim().is_empty() => t,
            _ => continue,
        };
        title_counts
            .entry(title.clone())
            .or_default()
            .insert(id.clone());
    }
    let mut title_to_session: HashMap<String, Option<String>> = HashMap::new();
    for (title, ids) in title_counts {
        let resolved = if ids.len() == 1 {
            ids.into_iter().next()
        } else {
            None
        };
        title_to_session.insert(title, resolved);
    }
    CliRead {
        events,
        title_to_session,
    }
}

/// `Math.max(0, finiteNumber(v) ?? 0)` for a token field, as a u64.
fn max0(value: Option<&Value>) -> u64 {
    value
        .and_then(finite_number)
        .map(|n| n.max(0.0) as u64)
        .unwrap_or(0)
}

/// The file stem with the `.ndjson` extension stripped, matching the TS
/// `file.replace(/^.*[\\/]/, "").replace(/\.ndjson$/, "")`.
fn ndjson_stem(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    base.strip_suffix(".ndjson").unwrap_or(base).to_owned()
}

/// Read the Desktop ACP `.ndjson` store. Faithful port of `readDesktop`.
fn read_desktop(
    root: &Path,
    title_to_session: &HashMap<String, Option<String>>,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let mut events: Vec<UsageEvent> = Vec::new();
    for file in list_files(root, |name| name.ends_with(".ndjson")) {
        let lines = match read_jsonl_lines(&file, skipped, warnings) {
            Some(l) => l,
            None => continue,
        };
        let stem = ndjson_stem(&file);
        // resolve the file's final title before emitting any event
        let mut title: Option<String> = None;
        for line in &lines {
            if !is_record(&line.value) {
                continue;
            }
            let notification = match line.value.get("notification") {
                Some(n) if is_record(n) => n,
                _ => continue,
            };
            if notification
                .get("sessionUpdate")
                .and_then(as_string)
                .as_deref()
                == Some("session_info_update")
            {
                title = notification.get("title").and_then(as_string).or(title);
            }
        }
        let resolved_session: Option<String> = match &title {
            Some(t) => title_to_session.get(t).cloned().flatten(),
            None => None,
        };

        let mut agg = TokenCounts {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            cache_write1h: None,
            reasoning: 0,
        };
        let mut saw_usage = false;
        let mut agg_model: Option<String> = None;
        let mut agg_timestamp: Option<String> = None;
        let mut legacy_index: usize = 0;

        for line in &lines {
            if !is_record(&line.value) {
                continue;
            }
            let notification = match line.value.get("notification") {
                Some(n) if is_record(n) => n,
                _ => continue,
            };
            let kind = notification.get("sessionUpdate").and_then(as_string);
            if kind.as_deref() == Some("session_info_update") {
                continue;
            }
            if kind.as_deref() == Some("usage_update") {
                let meta = match notification.get("_meta") {
                    Some(m) if is_record(m) => m,
                    _ => continue,
                };
                let input = max0(meta.get("cognition.ai/inputTokens"));
                let cache_read = max0(meta.get("cognition.ai/cachedReadTokens"));
                agg.input = agg.input.max(input.saturating_sub(cache_read));
                agg.cache_read = agg.cache_read.max(cache_read);
                agg.cache_write = agg
                    .cache_write
                    .max(max0(meta.get("cognition.ai/cachedWriteTokens")));
                agg.output += max0(meta.get("cognition.ai/outputTokens"));
                if agg_model.is_none() {
                    agg_model = resolve_model(meta.get("cognition.ai/model").and_then(as_string));
                }
                if agg_timestamp.is_none() {
                    agg_timestamp = parse_timestamp_to_iso(
                        notification.get("timestamp").unwrap_or(&Value::Null),
                    );
                }
                saw_usage = true;
                continue;
            }
            // legacy embedded-metrics event: one message per line
            let metrics = read_at_pointer(notification, &["content", "metadata", "metrics"])
                .or_else(|| read_at_pointer(notification, &["metadata", "metrics"]))
                .or_else(|| read_at_pointer(notification, &["metrics"]));
            let metrics = match metrics {
                Some(m) if is_record(m) => m,
                _ => continue,
            };
            let tokens = TokenCounts {
                input: max0(metrics.get("input_tokens")),
                output: max0(metrics.get("output_tokens")),
                cache_read: max0(metrics.get("cache_read_tokens")),
                cache_write: max0(metrics.get("cache_creation_tokens")),
                cache_write1h: None,
                reasoning: 0,
            };
            if tokens.input == 0
                && tokens.output == 0
                && tokens.cache_read == 0
                && tokens.cache_write == 0
            {
                legacy_index += 1;
                continue;
            }
            let model = resolve_model(
                read_at_pointer(notification, &["content", "metadata", "generation_model"])
                    .and_then(as_string),
            )
            .or_else(|| {
                resolve_model(
                    read_at_pointer(notification, &["metadata", "generation_model"])
                        .and_then(as_string),
                )
            });
            let legacy_ts = parse_ts_opt(read_at_pointer(
                notification,
                &["content", "metadata", "created_at"],
            ))
            .or_else(|| parse_ts_opt(read_at_pointer(notification, &["metadata", "created_at"])))
            .or_else(|| parse_ts_opt(notification.get("created_at")))
            .or_else(|| parse_ts_opt(notification.get("timestamp")));
            let legacy_ts = match legacy_ts {
                Some(t) => t,
                None => {
                    skipped.push(format!("{file}:{}", line.index + 1));
                    warnings.push(ReaderWarning {
                        message: format!(
                            "skipping devin desktop event with unusable timestamp {file}:{}\n",
                            line.index + 1
                        ),
                    });
                    legacy_index += 1;
                    continue;
                }
            };
            let session_id = resolved_session.clone().unwrap_or_else(|| stem.clone());
            events.push(UsageEvent {
                harness: "devin".to_owned(),
                timestamp: legacy_ts,
                session_id,
                message_id: format!("devin-desktop:{file}:{legacy_index}"),
                turn: true,
                subagent: false,
                model: format!(
                    "{DEVIN_PROVIDER}/{}",
                    model.unwrap_or_else(|| DEVIN_PROVIDER.to_owned())
                ),
                tokens,
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            });
            legacy_index += 1;
        }

        if saw_usage {
            if agg.input == 0 && agg.output == 0 && agg.cache_read == 0 && agg.cache_write == 0 {
                continue;
            }
            let agg_timestamp = match agg_timestamp {
                Some(t) => t,
                None => {
                    skipped.push(format!("{file}:usage"));
                    warnings.push(ReaderWarning {
                        message: format!(
                            "skipping devin desktop usage with unusable timestamp {file}:usage\n"
                        ),
                    });
                    continue;
                }
            };
            let session_id = resolved_session.clone().unwrap_or_else(|| stem.clone());
            events.push(UsageEvent {
                harness: "devin".to_owned(),
                timestamp: agg_timestamp,
                session_id,
                message_id: format!("devin-desktop:{file}:usage"),
                turn: false,
                subagent: false,
                model: format!(
                    "{DEVIN_PROVIDER}/{}",
                    agg_model.unwrap_or_else(|| DEVIN_PROVIDER.to_owned())
                ),
                tokens: agg,
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            });
        }
    }
    events
}

/// `parseTimestampToIso` for an optional value (None -> None).
fn parse_ts_opt(value: Option<&Value>) -> Option<String> {
    value.and_then(parse_timestamp_to_iso)
}

/// Read all devin usage events. Faithful port of `readDevin`.
pub fn read_devin(db_path: &Path, desktop_root: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen_sessions: HashSet<String> = HashSet::new();
    let mut title_to_session: HashMap<String, Option<String>> = HashMap::new();

    if db_path.exists() {
        let db_str = db_path.to_string_lossy().into_owned();
        let cli = read_cli(&db_str, &mut skipped, &mut warnings);
        title_to_session = cli.title_to_session;
        for event in cli.events {
            seen_sessions.insert(event.session_id.clone());
            events.push(event);
        }
    }
    for event in read_desktop(desktop_root, &title_to_session, &mut skipped, &mut warnings) {
        // the CLI db is authoritative; drop Desktop usage for a covered session
        if seen_sessions.contains(&event.session_id) {
            continue;
        }
        events.push(event);
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

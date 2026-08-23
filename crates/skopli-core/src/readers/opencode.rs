use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, dir_exists, file_stem_name, finite_number, is_record,
    list_files, normalize_workspace, parent_dir_name, read_json,
};
use super::sqlite_store::{
    ParsedValue, SqliteMessageStoreOptions, open_readonly, read_sqlite_message_store, table_columns,
};
use crate::time::to_iso_string;
use crate::types::{TokenCounts, UsageEvent};

/// The opencode reader wired into the harness registry. It reads both lanes the
/// TS `readOpencode` does: every `opencode*.db` SQLite store (authoritative)
/// first, then the legacy JSON store, with a per-harness message-id dedup so a
/// mid-migration mirror is not counted twice.
pub struct OpencodeReader;

impl Reader for OpencodeReader {
    fn harness_id(&self) -> &'static str {
        "opencode"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = opencode_roots(
            ctx.env("OPENCODE_DATA_DIR"),
            ctx.env("XDG_DATA_HOME"),
            ctx.home(),
        );
        read_opencode(&roots, "opencode")
    }
}

/// The MiMo Code reader wired into the harness registry. MiMo Code is an
/// opencode fork storing the same message format under the `mimocode` app dir,
/// so it shares the opencode logic; only the roots differ (env `MIMOCODE_HOME`
/// then `<home>/data`, else the XDG data dir `<xdgData>/mimocode`).
pub struct MimocodeReader;

impl Reader for MimocodeReader {
    fn harness_id(&self) -> &'static str {
        "mimocode"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = mimocode_roots(
            ctx.env("MIMOCODE_HOME"),
            ctx.env("XDG_DATA_HOME"),
            ctx.env("LOCALAPPDATA"),
            ctx.home(),
        );
        read_opencode(&roots, "mimocode")
    }
}

/// The Command Code reader wired into the harness registry. Command Code is an
/// opencode fork storing the same message format under the `commandcode` app
/// dir, so it shares the opencode logic; only the roots differ (the XDG data
/// dir `<xdgData>/commandcode`, else the per-OS data dirs used by Command Code
/// for `commandcode`).
pub struct CommandcodeReader;

impl Reader for CommandcodeReader {
    fn harness_id(&self) -> &'static str {
        "commandcode"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = commandcode_roots(
            ctx.env("XDG_DATA_HOME"),
            ctx.env("LOCALAPPDATA"),
            ctx.home(),
        );
        read_opencode(&roots, "commandcode")
    }
}

/// Resolve the opencode data roots. Faithful port of `opencodeRoots`.
pub fn opencode_roots(
    data_dir: Option<&str>,
    xdg_data_home: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(override_val) = data_dir
        && !override_val.is_empty()
    {
        return override_val
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    if let Some(xdg) = xdg_data_home
        && !xdg.is_empty()
    {
        return vec![PathBuf::from(xdg).join("opencode")];
    }
    vec![home.join(".local").join("share").join("opencode")]
}

/// Resolve the MiMo Code data roots. `MIMOCODE_HOME` is an absolute home whose
/// data dir is `<home>/data`; otherwise the XDG data dir (`<xdgData>/mimocode`,
/// with `LOCALAPPDATA` as the Windows XDG-data fallback), else
/// `<home>/.local/share/mimocode`.
pub fn mimocode_roots(
    mimocode_home: Option<&str>,
    xdg_data_home: Option<&str>,
    local_app_data: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(v) = mimocode_home.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v).join("data")];
    }
    if let Some(v) = xdg_data_home.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v).join("mimocode")];
    }
    if let Some(v) = local_app_data.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v).join("mimocode")];
    }
    vec![home.join(".local").join("share").join("mimocode")]
}

/// Resolve the Command Code data roots: the XDG data dir
/// (`<xdgData>/commandcode`), else the per-OS data dirs used by Command Code
/// (Linux `~/.local/share/commandcode`, macOS `~/Library/Application
/// Support/CommandCode`, and the Windows `%LOCALAPPDATA%\commandcode`
/// candidate).
pub fn commandcode_roots(
    xdg_data_home: Option<&str>,
    local_app_data: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(v) = xdg_data_home.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v).join("commandcode")];
    }
    let mut roots = vec![
        home.join(".local").join("share").join("commandcode"),
        home.join("Library")
            .join("Application Support")
            .join("CommandCode"),
    ];
    if let Some(v) = local_app_data.filter(|v| !v.is_empty()) {
        roots.push(PathBuf::from(v).join("commandcode"));
    }
    roots
}

#[derive(Clone, Default)]
struct SessionMeta {
    workspace: Option<String>,
    title: Option<String>,
}

struct SessionScan {
    subagents: HashMap<String, bool>,
    malformed: std::collections::HashSet<String>,
    meta: HashMap<String, SessionMeta>,
}

/// Faithful port of `scanSessions`.
fn scan_sessions(
    root: &Path,
    harness: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> SessionScan {
    let mut subagents = HashMap::new();
    let mut malformed = std::collections::HashSet::new();
    let mut meta = HashMap::new();
    let session_dir = root.join("storage").join("session");
    for file in list_files(&session_dir, |name| name.ends_with(".json")) {
        let mut mark_malformed = |skipped: &mut Vec<String>, warnings: &mut Vec<ReaderWarning>| {
            skipped.push(file.clone());
            malformed.insert(file_stem_name(&file));
            warnings.push(ReaderWarning {
                message: format!("skipping malformed {harness} session metadata {file}\n"),
            });
        };
        let record = match read_json(&file) {
            Some(r) => r,
            None => {
                mark_malformed(skipped, warnings);
                continue;
            }
        };
        if !is_record(&record) {
            mark_malformed(skipped, warnings);
            continue;
        }
        let id = match record.get("id").and_then(as_string) {
            Some(id) => id,
            None => {
                mark_malformed(skipped, warnings);
                continue;
            }
        };
        subagents.insert(
            id.clone(),
            record.get("parentID").and_then(as_string).is_some(),
        );
        meta.insert(
            id,
            SessionMeta {
                workspace: record.get("directory").and_then(normalize_workspace),
                title: record.get("title").and_then(as_string),
            },
        );
    }
    SessionScan {
        subagents,
        malformed,
        meta,
    }
}

struct Message {
    id: String,
    role: String,
    created: f64,
    event: Option<UsageEvent>,
}

struct ParsedMessage {
    message: Option<Message>,
    malformed: bool,
}

/// Faithful port of `parseMessage`. `authoritative_created` mirrors the TS
/// optional param: when `Some`, it overrides the `time.created` lookup (the
/// SQLite lane passes the `time_created` column through here).
fn parse_message(
    raw: &Value,
    harness: &str,
    session_id: &str,
    subagent: bool,
    meta: Option<&SessionMeta>,
    authoritative_created: Option<f64>,
) -> ParsedMessage {
    if !is_record(raw) {
        return ParsedMessage {
            message: None,
            malformed: true,
        };
    }
    let id = raw.get("id").and_then(as_string);
    let role = raw.get("role").and_then(as_string);
    let time = raw.get("time");
    let created = match authoritative_created {
        Some(c) => Some(c),
        None => match time {
            Some(t) if is_record(t) => t.get("created").and_then(finite_number),
            _ => None,
        },
    };
    let (id, role, created) = match (id, role, created) {
        (Some(i), Some(r), Some(c)) => (i, r, c),
        _ => {
            return ParsedMessage {
                message: None,
                malformed: true,
            };
        }
    };
    if role != "assistant" {
        return ParsedMessage {
            message: Some(Message {
                id,
                role,
                created,
                event: None,
            }),
            malformed: false,
        };
    }
    let tokens = raw.get("tokens");
    let provider_id = raw.get("providerID").and_then(as_string);
    let model_id = raw.get("modelID").and_then(as_string);
    let tokens = match (tokens, &provider_id, &model_id) {
        (Some(t), Some(_), Some(_)) if is_record(t) => t,
        _ => {
            return ParsedMessage {
                message: None,
                malformed: true,
            };
        }
    };
    let cache = tokens.get("cache");
    let input = tokens.get("input").and_then(finite_number);
    let output = tokens.get("output").and_then(finite_number);
    let cache_read = match cache {
        Some(c) if is_record(c) => c.get("read").and_then(finite_number),
        _ => None,
    };
    let cache_write = match cache {
        Some(c) if is_record(c) => c.get("write").and_then(finite_number),
        _ => None,
    };
    let (input, output, cache_read, cache_write) = match (input, output, cache_read, cache_write) {
        (Some(i), Some(o), Some(cr), Some(cw)) => (i, o, cr, cw),
        _ => {
            return ParsedMessage {
                message: None,
                malformed: true,
            };
        }
    };
    let cost = raw.get("cost").and_then(finite_number);
    let event = UsageEvent {
        harness: harness.to_owned(),
        timestamp: to_iso_string(created as i64).unwrap_or_default(),
        session_id: session_id.to_owned(),
        message_id: id.clone(),
        turn: false,
        subagent,
        model: format!("{}/{}", provider_id.unwrap(), model_id.unwrap()),
        tokens: TokenCounts {
            input: input as u64,
            output: output as u64,
            cache_read: cache_read as u64,
            cache_write: cache_write as u64,
            cache_write1h: None,
            reasoning: tokens
                .get("reasoning")
                .and_then(finite_number)
                .unwrap_or(0.0) as u64,
        },
        calls: None,
        cost_usd: match cost {
            Some(c) if c != 0.0 => Some(c),
            _ => None,
        },
        workspace: meta.and_then(|m| m.workspace.clone()),
        title: meta.and_then(|m| m.title.clone()),
    };
    ParsedMessage {
        message: Some(Message {
            id,
            role,
            created,
            event: Some(event),
        }),
        malformed: false,
    }
}

/// Faithful port of `readJsonStore`.
fn read_json_store(root: &Path, harness: &str) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let scan = scan_sessions(root, harness, &mut skipped, &mut warnings);
    let message_dir = root.join("storage").join("message");
    if !dir_exists(&message_dir) {
        return ReaderResult {
            events,
            skipped,
            warnings,
        };
    }
    // Group message files by session, preserving discovery (sorted) order.
    let mut order: Vec<String> = Vec::new();
    let mut by_session: HashMap<String, Vec<String>> = HashMap::new();
    for file in list_files(&message_dir, |name| {
        name.starts_with("msg_") && name.ends_with(".json")
    }) {
        let session_id = parent_dir_name(&file);
        if scan.malformed.contains(&session_id) {
            continue;
        }
        if !by_session.contains_key(&session_id) {
            order.push(session_id.clone());
        }
        by_session.entry(session_id).or_default().push(file);
    }
    for session_id in &order {
        let files = &by_session[session_id];
        let mut messages: Vec<Message> = Vec::new();
        for file in files {
            let raw = match read_json(file) {
                Some(r) => r,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping unreadable {harness} message {file}\n"),
                    });
                    skipped.push(file.clone());
                    continue;
                }
            };
            let subagent = scan.subagents.get(session_id).copied().unwrap_or(false);
            let result = parse_message(
                &raw,
                harness,
                session_id,
                subagent,
                scan.meta.get(session_id),
                None,
            );
            if result.malformed {
                skipped.push(file.clone());
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed {harness} message {file}\n"),
                });
            } else if let Some(message) = result.message {
                messages.push(message);
            }
        }
        messages.sort_by(|a, b| {
            a.created
                .partial_cmp(&b.created)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut prev_was_user = false;
        for message in messages {
            let role = message.role.clone();
            if let Some(mut event) = message.event {
                event.turn = prev_was_user;
                events.push(event);
            }
            prev_was_user = role == "user";
        }
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

/// Read the per-session metadata table from a `.db`. Faithful port of
/// `readDbSessionMeta`: only present columns are selected, an id-less schema
/// yields empty meta.
fn read_db_session_meta(db_path: &str) -> HashMap<String, SessionMeta> {
    let mut meta = HashMap::new();
    let conn = match open_readonly(db_path) {
        Ok(c) => c,
        Err(_) => return meta,
    };
    let columns = table_columns(&conn, "session");
    if !columns.contains("id") {
        return meta;
    }
    let has_directory = columns.contains("directory");
    let has_title = columns.contains("title");
    if !has_directory && !has_title {
        return meta;
    }
    let mut selected = vec!["id".to_owned()];
    if has_directory {
        selected.push("directory".to_owned());
    }
    if has_title {
        selected.push("title".to_owned());
    }
    let query = format!("SELECT {} FROM session", selected.join(", "));
    let rows = match super::sqlite_store::query_rows(&conn, &query) {
        Ok(r) => r,
        Err(_) => return meta,
    };
    for row in &rows {
        if !is_record(row) {
            continue;
        }
        let id = match row.get("id").and_then(as_string) {
            Some(id) => id,
            None => continue,
        };
        meta.insert(
            id,
            SessionMeta {
                workspace: if has_directory {
                    row.get("directory").and_then(normalize_workspace)
                } else {
                    None
                },
                title: if has_title {
                    row.get("title").and_then(as_string)
                } else {
                    None
                },
            },
        );
    }
    meta
}

/// Enrich a v2 (`session_message`) row's data object with the row's role +
/// model split, mirroring the TS `enriched` construction.
fn enrich_v2(raw: &Value, message_id: &str, row_role: Option<&str>) -> Value {
    let mut obj = match raw {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    obj.insert("id".to_owned(), Value::String(message_id.to_owned()));
    if let Some(role) = row_role {
        obj.insert("role".to_owned(), Value::String(role.to_owned()));
        let data_model = raw.get("model");
        let provider = data_model
            .filter(|m| is_record(m))
            .and_then(|m| m.get("providerID").and_then(as_string))
            .or_else(|| raw.get("providerID").and_then(as_string));
        let model_id = data_model
            .filter(|m| is_record(m))
            .and_then(|m| m.get("id").and_then(as_string))
            .or_else(|| raw.get("modelID").and_then(as_string));
        match provider {
            Some(p) => {
                obj.insert("providerID".to_owned(), Value::String(p));
            }
            None => {
                obj.remove("providerID");
            }
        }
        match model_id {
            Some(m) => {
                obj.insert("modelID".to_owned(), Value::String(m));
            }
            None => {
                obj.remove("modelID");
            }
        }
    }
    Value::Object(obj)
}

/// Read one opencode `.db`, running both the v1 (`message`) and v2
/// (`session_message`) queries and merging their sessions. Faithful port of
/// `readDbRows`. Returns `Err` only when BOTH queries fail to open/run (the TS
/// `stores.length === 0` throw).
fn read_db_rows(db_path: &str, harness: &str) -> Result<ReaderResult, ()> {
    let session_meta = read_db_session_meta(db_path);
    let malformed_label = format!("{harness} message");

    struct DbValue {
        session_id: String,
        message: Message,
    }

    let mut parse = |row: &Value, raw: &Value| -> ParsedValue<DbValue> {
        let message_id = row.get("id").and_then(as_string);
        let session_id = row.get("session_id").and_then(as_string);
        let (message_id, session_id) = match (message_id, session_id) {
            (Some(m), Some(s)) => (m, s),
            _ => {
                return ParsedValue {
                    value: None,
                    malformed: true,
                };
            }
        };
        let subagent = row.get("parent_id").and_then(as_string).is_some();
        if !is_record(raw) {
            return ParsedValue {
                value: None,
                malformed: true,
            };
        }
        let time = raw.get("time");
        let time_created = row
            .get("time_created")
            .and_then(finite_number)
            .or_else(|| match time {
                Some(t) if is_record(t) => t.get("created").and_then(finite_number),
                _ => None,
            });
        let time_created = match time_created {
            Some(t) => t,
            None => {
                return ParsedValue {
                    value: None,
                    malformed: true,
                };
            }
        };
        let row_role = row.get("type").and_then(as_string);
        let enriched = enrich_v2(raw, &message_id, row_role.as_deref());
        let message = parse_message(
            &enriched,
            harness,
            &session_id,
            subagent,
            session_meta.get(&session_id),
            Some(time_created),
        );
        ParsedValue {
            value: message.message.map(|m| DbValue {
                session_id,
                message: m,
            }),
            malformed: message.malformed,
        }
    };

    let queries = [
        "SELECT message.id, message.session_id, message.time_created, message.data, \
         session.parent_id \
         FROM message JOIN session ON session.id = message.session_id \
         ORDER BY message.time_created, message.id",
        "SELECT session_message.id, session_message.session_id, session_message.data, \
         session_message.type, session.parent_id \
         FROM session_message JOIN session ON session.id = session_message.session_id \
         ORDER BY session_message.id",
    ];

    let mut stores: Vec<(Vec<DbValue>, ReaderResult)> = Vec::new();
    for query in queries {
        if let Ok(store) = read_sqlite_message_store(SqliteMessageStoreOptions {
            db_path,
            query,
            malformed_label: &malformed_label,
            parse: &mut parse,
        }) {
            stores.push(store);
        }
    }
    if stores.is_empty() {
        return Err(());
    }

    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut order: Vec<String> = Vec::new();
    let mut by_session: HashMap<String, Vec<Message>> = HashMap::new();
    for (values, result) in stores {
        skipped.extend(result.skipped);
        warnings.extend(result.warnings);
        for value in values {
            if !by_session.contains_key(&value.session_id) {
                order.push(value.session_id.clone());
            }
            by_session
                .entry(value.session_id)
                .or_default()
                .push(value.message);
        }
    }
    let mut events: Vec<UsageEvent> = Vec::new();
    for session_id in &order {
        let mut messages = by_session.remove(session_id).unwrap_or_default();
        messages.sort_by(|a, b| {
            a.created
                .partial_cmp(&b.created)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut prev_was_user = false;
        for message in messages {
            let role = message.role.clone();
            if let Some(mut event) = message.event {
                event.turn = prev_was_user;
                events.push(event);
            }
            prev_was_user = role == "user";
        }
    }
    Ok(ReaderResult {
        events,
        skipped,
        warnings,
    })
}

/// List the `opencode.db` / `opencode-*.db` files directly under `root`, sorted.
/// Faithful port of the inline `dbPaths` reader in `readOpencode`.
fn opencode_db_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let matches = name == "opencode.db"
                || (name.starts_with("opencode-") && name.ends_with(".db") && name.len() > 12);
            if matches {
                paths.push(entry.path());
            }
        }
    }
    paths.sort();
    paths
}

/// Read all opencode usage events: every `.db` across every root first (so
/// authoritative db rows win the message-id dedup), then the legacy JSON store.
/// Faithful port of `readOpencode`.
pub fn read_opencode(roots: &[PathBuf], harness: &str) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let merge = |result: ReaderResult,
                 skipped: &mut Vec<String>,
                 warnings: &mut Vec<ReaderWarning>,
                 events: &mut Vec<UsageEvent>,
                 seen: &mut std::collections::HashSet<String>| {
        skipped.extend(result.skipped);
        warnings.extend(result.warnings);
        for event in result.events {
            if seen.contains(&event.message_id) {
                continue;
            }
            seen.insert(event.message_id.clone());
            events.push(event);
        }
    };

    for root in roots {
        for db_path in opencode_db_paths(root) {
            let db_str = db_path.to_string_lossy().into_owned();
            match read_db_rows(&db_str, harness) {
                Ok(result) => merge(result, &mut skipped, &mut warnings, &mut events, &mut seen),
                Err(()) => {
                    warnings.push(ReaderWarning {
                        message: format!("unable to read {harness} database {db_str}\n"),
                    });
                    skipped.push(db_str);
                }
            }
        }
    }
    for root in roots {
        merge(
            read_json_store(root, harness),
            &mut skipped,
            &mut warnings,
            &mut events,
            &mut seen,
        );
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/u")
    }

    #[test]
    fn mimocode_home_appends_data() {
        let roots = mimocode_roots(Some("/custom/mimo"), None, None, &home());
        assert_eq!(roots, vec![PathBuf::from("/custom/mimo").join("data")]);
    }

    #[test]
    fn mimocode_xdg_data_home_appends_app_dir() {
        let roots = mimocode_roots(None, Some("/xdg/data"), None, &home());
        assert_eq!(roots, vec![PathBuf::from("/xdg/data").join("mimocode")]);
    }

    #[test]
    fn mimocode_home_wins_over_xdg_and_local_app_data() {
        let roots = mimocode_roots(
            Some("/custom/mimo"),
            Some("/xdg/data"),
            Some("/local"),
            &home(),
        );
        assert_eq!(roots, vec![PathBuf::from("/custom/mimo").join("data")]);
    }

    #[test]
    fn mimocode_xdg_wins_over_local_app_data() {
        let roots = mimocode_roots(None, Some("/xdg/data"), Some("/local"), &home());
        assert_eq!(roots, vec![PathBuf::from("/xdg/data").join("mimocode")]);
    }

    #[test]
    fn mimocode_local_app_data_windows_fallback() {
        let roots = mimocode_roots(None, None, Some("/local"), &home());
        assert_eq!(roots, vec![PathBuf::from("/local").join("mimocode")]);
    }

    #[test]
    fn mimocode_default_home_without_overrides() {
        let roots = mimocode_roots(None, None, None, &home());
        assert_eq!(
            roots,
            vec![home().join(".local").join("share").join("mimocode")]
        );
    }

    #[test]
    fn mimocode_empty_overrides_fall_through_to_default() {
        let roots = mimocode_roots(Some(""), Some(""), Some(""), &home());
        assert_eq!(
            roots,
            vec![home().join(".local").join("share").join("mimocode")]
        );
    }

    #[test]
    fn commandcode_xdg_data_home_appends_app_dir() {
        let roots = commandcode_roots(Some("/xdg/data"), None, &home());
        assert_eq!(roots, vec![PathBuf::from("/xdg/data").join("commandcode")]);
    }

    #[test]
    fn commandcode_xdg_wins_over_local_app_data() {
        let roots = commandcode_roots(Some("/xdg/data"), Some("/local"), &home());
        assert_eq!(roots, vec![PathBuf::from("/xdg/data").join("commandcode")]);
    }

    #[test]
    fn commandcode_default_candidates_include_all_os_dirs() {
        let roots = commandcode_roots(None, Some("/local"), &home());
        assert_eq!(
            roots,
            vec![
                home().join(".local").join("share").join("commandcode"),
                home()
                    .join("Library")
                    .join("Application Support")
                    .join("CommandCode"),
                PathBuf::from("/local").join("commandcode"),
            ]
        );
    }

    #[test]
    fn commandcode_default_without_local_app_data() {
        let roots = commandcode_roots(None, None, &home());
        assert_eq!(
            roots,
            vec![
                home().join(".local").join("share").join("commandcode"),
                home()
                    .join("Library")
                    .join("Application Support")
                    .join("CommandCode"),
            ]
        );
    }

    #[test]
    fn commandcode_empty_xdg_falls_through_to_default() {
        let roots = commandcode_roots(Some(""), None, &home());
        assert_eq!(
            roots,
            vec![
                home().join(".local").join("share").join("commandcode"),
                home()
                    .join("Library")
                    .join("Application Support")
                    .join("CommandCode"),
            ]
        );
    }
}

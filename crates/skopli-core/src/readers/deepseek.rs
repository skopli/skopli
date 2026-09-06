use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    EpochUnit, ReaderResult, ReaderWarning, as_string, dir_exists, epoch_to_iso_unit,
    finite_number, is_record, list_files,
};
use super::sqlite_store::{blob_column, open_readonly, pragma_int, table_exists};
use crate::types::{TokenCounts, UsageEvent};

/// The DeepSeek Harness reader wired into the harness registry. DeepSeek
/// Harness stores each session as an append-only JSONL event log under
/// `~/.dsh/<project>/<session>/session.jsonl`, optionally Zstandard-compressed
/// as `session.jsonl.zstd`. The first line is a `{type:'session', id,
/// createdAt, parentSession?, origin?, ...}` header; every later line is a
/// `{type, seq, time, data}` event, each carrying its own epoch-millisecond
/// `time`. Token usage rides the finalized `assistant/message` events
/// (`data.usage`), whose `data.message.source.model` names the model; the
/// early `assistant/chunk` usage sample for the same step is superseded by that
/// message and is not read, so per-step usage is never double counted. The
/// `~/.dsh` root is overridable through `DSH_HOME`.
///
/// DeepSeek Harness also ships an optional SQLite persistence backend (schema
/// 17, application id `0x44534850`). It stores the same `SessionEvent`
/// vocabulary in an `events(session_id, seq, type, time, data, ...)` table,
/// where `data` is the event's JSON (Zstandard-compressed as a blob at or above
/// 4096 bytes). The two backends are mounted through one `sessionPersistence`
/// seam, so a deployment normally runs exactly one; this reader still reads any
/// DeepSeek session database found under the root (identified by its reserved
/// `application_id`, since the DB path is deployment-configured with no fixed
/// filename) and deduplicates against the JSONL backend by the logical
/// `session_id:seq` identity, JSONL winning on collision.
pub struct DeepseekReader;

impl Reader for DeepseekReader {
    fn harness_id(&self) -> &'static str {
        "deepseek"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let home = deepseek_home(ctx.env("DSH_HOME"), ctx.home());
        read_deepseek(&home)
    }
}

const HARNESS: &str = "deepseek";
const UNKNOWN_MODEL: &str = "deepseek-unknown";

/// Resolve the DeepSeek Harness home. `DSH_HOME` wins when set; otherwise
/// `~/.dsh`.
pub fn deepseek_home(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    home.join(".dsh")
}

/// A required token cell (`inputTokens`/`outputTokens` are non-optional in the
/// upstream `TokenUsage`). The cell must be a non-negative finite number; an
/// absent, `null`, or invalid cell marks the usage malformed.
fn required_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// An optional token cell (`cacheReadTokens`/`cacheWriteTokens`/`reasoningTokens`
/// are `?` in the upstream `TokenUsage`). An absent or `null` cell is `0`; a
/// populated cell that is not a non-negative finite number marks the usage
/// malformed.
fn optional_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// The outcome of parsing one `assistant/message` `data.usage` map.
enum UsageOutcome {
    /// A populated, valid usage map with a non-zero total.
    Counts(TokenCounts),
    /// A valid usage map whose token counts are all zero (dropped, not warned).
    Zero,
    /// The usage map is absent or a populated cell is invalid.
    Malformed,
}

/// Parse an `assistant/message` `data.usage`. `inputTokens` is already the
/// uncached input (the DeepSeek adapter subtracts cache reads before writing
/// it), so it maps straight to `input` with no further subtraction; likewise
/// `outputTokens` is completion excluding reasoning. Cache and reasoning cells
/// default to 0 when absent.
fn parse_usage(usage: &Value) -> UsageOutcome {
    if !is_record(usage) {
        return UsageOutcome::Malformed;
    }
    let cells = [
        required_cell(usage.get("inputTokens")),
        required_cell(usage.get("outputTokens")),
        optional_cell(usage.get("cacheReadTokens")),
        optional_cell(usage.get("cacheWriteTokens")),
        optional_cell(usage.get("reasoningTokens")),
    ];
    let [input, output, cache_read, cache_write, reasoning] = match cells {
        [Some(i), Some(o), Some(cr), Some(cw), Some(r)] => [i, o, cr, cw, r],
        _ => return UsageOutcome::Malformed,
    };
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 && reasoning == 0 {
        return UsageOutcome::Zero;
    }
    UsageOutcome::Counts(TokenCounts {
        input,
        output,
        cache_read,
        cache_write,
        cache_write1h: None,
        reasoning,
    })
}

/// The session-level fields read from the `{type:'session'}` header line.
#[derive(Default)]
struct SessionHeader {
    id: Option<String>,
    subagent: bool,
    workspace: Option<String>,
}

/// Parse the header line into a [`SessionHeader`]. A header without an `id`
/// falls back to the file's parent directory name at the call site.
fn parse_header(value: &Value) -> SessionHeader {
    let id = value
        .get("id")
        .and_then(as_string)
        .filter(|s| !s.trim().is_empty());
    let origin_subagent = value.get("origin").and_then(Value::as_str) == Some("subagent");
    let depth_subagent = value
        .get("delegationDepth")
        .and_then(finite_number)
        .map(|d| d > 0.0)
        .unwrap_or(false);
    let parent_subagent = value
        .get("parentSession")
        .and_then(as_string)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let workspace = value
        .get("cwd")
        .and_then(super::shared::normalize_workspace);
    SessionHeader {
        id,
        subagent: origin_subagent || depth_subagent || parent_subagent,
        workspace,
    }
}

/// The per-event model, read from `data.message.source.model`.
fn event_model(data: &Value) -> String {
    data.get("message")
        .and_then(|m| m.get("source"))
        .and_then(|s| s.get("model"))
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| UNKNOWN_MODEL.to_owned())
}

/// The logical dedup identity for one event: `<session_id>:<seq>`. Both
/// backends emit the same `(session_id, seq)` for the same event, so a JSONL
/// log and a SQLite store that describe the same session never double count.
/// A missing or non-integer `seq` yields `None` (never deduped).
fn dedup_key(session_id: &str, seq: Option<&Value>) -> Option<String> {
    let seq = seq.and_then(finite_number)?;
    if seq < 0.0 || seq.fract() != 0.0 {
        return None;
    }
    Some(format!("{session_id}:{}", seq as u64))
}

/// Read the raw bytes of a `session.jsonl` (plaintext) or `session.jsonl.zstd`
/// (Zstandard) file to a UTF-8 string. A missing/unreadable file or a
/// malformed/truncated zstd stream yields `None`, which the caller turns into
/// the standard file-level malformed diagnostic.
fn read_log_text(file: &str) -> Option<String> {
    let bytes = std::fs::read(file).ok()?;
    if file.ends_with(".zstd") {
        let mut decoder = ruzstd::decoding::StreamingDecoder::new(bytes.as_slice()).ok()?;
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).ok()?;
        String::from_utf8(out).ok()
    } else {
        String::from_utf8(bytes).ok()
    }
}

/// Read one session log (already decompressed to text) into usage events.
/// Each emitted event's logical `session_id:seq` identity is inserted into
/// `seen` so the SQLite lane can skip a duplicate of the same event.
fn read_session(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
    seen: &mut HashSet<String>,
) -> Vec<UsageEvent> {
    let mut events: Vec<UsageEvent> = Vec::new();
    let text = match read_log_text(file) {
        Some(t) => t,
        None => {
            warnings.push(ReaderWarning {
                message: format!("skipping malformed {HARNESS} file {file}\n"),
            });
            skipped.push(file.to_owned());
            return events;
        }
    };

    let mut header = SessionHeader::default();
    let fallback_id = super::shared::parent_dir_name(file);

    for (index, raw) in text.split('\n').enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                let loc = format!("{file}:{}", index + 1);
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed line {loc}\n"),
                });
                skipped.push(loc);
                continue;
            }
        };
        if !is_record(&value) {
            continue;
        }
        let record_type = value.get("type").and_then(Value::as_str);
        if record_type == Some("session") {
            if header.id.is_none() {
                header = parse_header(&value);
            }
            continue;
        }
        if record_type != Some("assistant/message") {
            continue;
        }
        let data = match value.get("data") {
            Some(d) if is_record(d) => d,
            _ => continue,
        };
        let usage = match data.get("usage") {
            Some(u) => u,
            None => continue,
        };
        let tokens = match parse_usage(usage) {
            UsageOutcome::Counts(t) => t,
            UsageOutcome::Zero => continue,
            UsageOutcome::Malformed => {
                let loc = format!("{file}:{}", index + 1);
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed {HARNESS} record {loc}\n"),
                });
                skipped.push(loc);
                continue;
            }
        };
        let timestamp = match value.get("time").and_then(|t| {
            if t.is_number() {
                epoch_to_iso_unit(t, EpochUnit::Ms)
            } else {
                None
            }
        }) {
            Some(ts) => ts,
            None => {
                let loc = format!("{file}:{}", index + 1);
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed {HARNESS} record {loc}\n"),
                });
                skipped.push(loc);
                continue;
            }
        };
        let session_id = header.id.clone().unwrap_or_else(|| fallback_id.clone());
        if let Some(key) = dedup_key(&session_id, value.get("seq")) {
            seen.insert(key);
        }
        events.push(UsageEvent {
            harness: HARNESS.to_owned(),
            timestamp,
            session_id,
            message_id: format!("{file}:{index}"),
            turn: true,
            subagent: header.subagent,
            model: event_model(data),
            tokens,
            calls: None,
            cost_usd: None,
            workspace: header.workspace.clone(),
            title: None,
        });
    }
    events
}

/// The reserved SQLite `application_id` for DeepSeek Harness session databases
/// (`0x44534850`, ASCII "DSHP") and the schema version this build understands.
const SQLITE_APPLICATION_ID: i64 = 0x4453_4850;
const SQLITE_SCHEMA_VERSION: i64 = 17;

/// True when this SQLite connection is a DeepSeek Harness session database:
/// its `application_id` matches the reserved id and its `user_version` is the
/// known schema version. This identity probe is how the reader recognizes a
/// DeepSeek session store without a fixed filename (the DB path is
/// deployment-configured).
fn is_session_database(conn: &rusqlite::Connection) -> bool {
    pragma_int(conn, "application_id") == Some(SQLITE_APPLICATION_ID)
        && pragma_int(conn, "user_version") == Some(SQLITE_SCHEMA_VERSION)
        && table_exists(conn, "sessions")
        && table_exists(conn, "events")
}

/// Decode one `events.data` cell to its JSON value. `data` is plaintext JSON
/// below 4096 bytes, else a Zstandard-compressed blob; both decode to the same
/// event JSON. A NULL cell, a non-text/blob cell, a malformed zstd stream, or
/// invalid JSON yields `None`.
fn decode_event_data(cell: rusqlite::types::ValueRef<'_>) -> Option<Value> {
    let bytes = blob_column(cell)?;
    let text = if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        let mut decoder = ruzstd::decoding::StreamingDecoder::new(bytes.as_slice()).ok()?;
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).ok()?;
        String::from_utf8(out).ok()?
    } else {
        String::from_utf8(bytes).ok()?
    };
    serde_json::from_str(&text).ok()
}

/// The session-level fields read from a `sessions` row, keyed by session id.
struct SqliteSession {
    subagent: bool,
    workspace: Option<String>,
}

/// Read the `sessions` table into a map from session id to its header fields.
/// Mirrors [`parse_header`] for the SQLite column shape.
fn read_sqlite_sessions(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<std::collections::HashMap<String, SqliteSession>> {
    let mut map = std::collections::HashMap::new();
    let mut stmt =
        conn.prepare("SELECT id, cwd, parent_session, origin, delegation_depth FROM sessions")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = match row.get::<_, Option<String>>(0)? {
            Some(id) if !id.trim().is_empty() => id,
            _ => continue,
        };
        let cwd = row.get::<_, Option<String>>(1)?;
        let origin_subagent = row.get::<_, Option<String>>(3)?.as_deref() == Some("subagent");
        let parent_subagent = row
            .get::<_, Option<String>>(2)?
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let depth_subagent = row
            .get::<_, Option<i64>>(4)?
            .map(|d| d > 0)
            .unwrap_or(false);
        let workspace = cwd
            .as_deref()
            .and_then(|c| super::shared::normalize_workspace(&Value::String(c.to_owned())));
        map.insert(
            id,
            SqliteSession {
                subagent: origin_subagent || parent_subagent || depth_subagent,
                workspace,
            },
        );
    }
    Ok(map)
}

/// Read one DeepSeek Harness SQLite session database into usage events. Only
/// scalar `assistant/message` rows carry `data.usage`; packed chunk rows never
/// do, mirroring how the JSONL lane ignores `assistant/chunk` usage samples.
/// An event whose logical `session_id:seq` identity is already in `seen` (from
/// the JSONL lane) is skipped so the two backends never double count.
fn read_sqlite_db(
    db_str: &str,
    seen: &mut HashSet<String>,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let mut events: Vec<UsageEvent> = Vec::new();
    let conn = match open_readonly(db_str) {
        Ok(c) => c,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read {HARNESS} database {db_str}: {e}\n"),
            });
            skipped.push(db_str.to_owned());
            return events;
        }
    };
    if let Err(e) = conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(())) {
        warnings.push(ReaderWarning {
            message: format!("unable to read {HARNESS} database {db_str}: {e}\n"),
        });
        skipped.push(db_str.to_owned());
        return events;
    }
    if !is_session_database(&conn) {
        return events;
    }
    let sessions = match read_sqlite_sessions(&conn) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read {HARNESS} database {db_str}: {e}\n"),
            });
            skipped.push(db_str.to_owned());
            return events;
        }
    };
    let mut stmt = match conn
        .prepare("SELECT session_id, seq, type, time, data FROM events WHERE type = 'assistant/message' ORDER BY session_id, seq")
    {
        Ok(s) => s,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read {HARNESS} database {db_str}: {e}\n"),
            });
            skipped.push(db_str.to_owned());
            return events;
        }
    };
    let mut rows = match stmt.query([]) {
        Ok(r) => r,
        Err(e) => {
            warnings.push(ReaderWarning {
                message: format!("unable to read {HARNESS} database {db_str}: {e}\n"),
            });
            skipped.push(db_str.to_owned());
            return events;
        }
    };

    let mut ordinal: usize = 0;
    loop {
        let row = match rows.next() {
            Ok(Some(row)) => row,
            Ok(None) => break,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read {HARNESS} database {db_str}: {e}\n"),
                });
                skipped.push(db_str.to_owned());
                break;
            }
        };
        ordinal += 1;
        let session_id: String = match row.get::<_, Option<String>>(0) {
            Ok(Some(id)) if !id.trim().is_empty() => id,
            _ => continue,
        };
        let seq: Option<i64> = row.get::<_, Option<i64>>(1).unwrap_or(None);
        let label = format!("{db_str}:{ordinal}");
        let data = match row.get_ref(4).ok().and_then(decode_event_data) {
            Some(d) if is_record(&d) => d,
            _ => {
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed {HARNESS} record {label}\n"),
                });
                skipped.push(label);
                continue;
            }
        };
        let usage = match data.get("usage") {
            Some(u) => u,
            None => continue,
        };
        let tokens = match parse_usage(usage) {
            UsageOutcome::Counts(t) => t,
            UsageOutcome::Zero => continue,
            UsageOutcome::Malformed => {
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed {HARNESS} record {label}\n"),
                });
                skipped.push(label);
                continue;
            }
        };
        let time = row.get_ref(3).ok().map(super::sqlite_store::cell_to_value);
        let timestamp = match time.as_ref().and_then(|t| {
            if t.is_number() {
                epoch_to_iso_unit(t, EpochUnit::Ms)
            } else {
                None
            }
        }) {
            Some(ts) => ts,
            None => {
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed {HARNESS} record {label}\n"),
                });
                skipped.push(label);
                continue;
            }
        };
        let dedup = seq.filter(|s| *s >= 0).map(|s| format!("{session_id}:{s}"));
        if let Some(key) = &dedup
            && seen.contains(key)
        {
            continue;
        }
        let session = sessions.get(&session_id);
        let subagent = session.map(|s| s.subagent).unwrap_or(false);
        let workspace = session.and_then(|s| s.workspace.clone());
        let message_id = match seq {
            Some(s) => format!("{session_id}:{s}"),
            None => session_id.clone(),
        };
        if let Some(key) = dedup {
            seen.insert(key);
        }
        events.push(UsageEvent {
            harness: HARNESS.to_owned(),
            timestamp,
            session_id,
            message_id,
            turn: true,
            subagent,
            model: event_model(&data),
            tokens,
            calls: None,
            cost_usd: None,
            workspace,
            title: None,
        });
    }
    events
}

/// Read all DeepSeek Harness usage events reachable from `home`. Every
/// `session.jsonl` / `session.jsonl.zstd` under `<home>` (walked recursively,
/// since sessions nest under project directories) is read; one event is emitted
/// per finalized `assistant/message` that carries usage. Any SQLite session
/// database under `<home>` (identified by its reserved `application_id`) is read
/// too; its events dedupe against the JSONL lane by the logical `session_id:seq`
/// identity, JSONL winning on collision.
pub fn read_deepseek(home: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if !dir_exists(home) {
        return ReaderResult {
            events,
            skipped,
            warnings,
        };
    }

    let files = list_files(home, |name| {
        name == "session.jsonl" || name == "session.jsonl.zstd"
    });
    for file in &files {
        events.extend(read_session(file, &mut skipped, &mut warnings, &mut seen));
    }

    let databases = list_files(home, |name| {
        name.ends_with(".db") || name.ends_with(".sqlite") || name.ends_with(".sqlite3")
    });
    for db in &databases {
        events.extend(read_sqlite_db(db, &mut seen, &mut skipped, &mut warnings));
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
    use std::fs;

    fn home() -> PathBuf {
        PathBuf::from("/home/u")
    }

    #[test]
    fn default_home_resolves_dot_dsh() {
        assert_eq!(deepseek_home(None, &home()), home().join(".dsh"));
    }

    #[test]
    fn dsh_home_override_wins() {
        assert_eq!(
            deepseek_home(Some("/custom/dsh"), &home()),
            PathBuf::from("/custom/dsh")
        );
    }

    #[test]
    fn empty_override_falls_through_to_default() {
        assert_eq!(deepseek_home(Some(""), &home()), home().join(".dsh"));
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-deepseek-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn write_session(dir: &Path, project: &str, session: &str, body: &str) -> PathBuf {
        let session_dir = dir.join(project).join(session);
        fs::create_dir_all(&session_dir).unwrap();
        let path = session_dir.join("session.jsonl");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn empty_home_yields_no_events_then_reads_after_sessions_appear() {
        let dir = tmp_dir("presence");
        assert!(read_deepseek(&dir).events.is_empty());
        write_session(
            &dir,
            "--proj--",
            "s1",
            "{\"type\":\"session\",\"version\":0,\"id\":\"sess-a\",\"createdAt\":1754042400000,\"delegationDepth\":0}\n\
             {\"type\":\"assistant/message\",\"seq\":1,\"time\":1754042401000,\"data\":{\"turn\":0,\"step\":0,\"message\":{\"role\":\"assistant\",\"source\":{\"kind\":\"model\",\"model\":\"deepseek-chat\"}},\"usage\":{\"inputTokens\":10,\"outputTokens\":5}}}\n",
        );
        let events = read_deepseek(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, "sess-a");
        assert_eq!(events[0].model, "deepseek-chat");
        assert_eq!(events[0].timestamp, "2025-08-01T10:00:01.000Z");
        assert_eq!(events[0].tokens.input, 10);
        assert_eq!(events[0].tokens.output, 5);
        assert!(!events[0].subagent);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cache_and_reasoning_cells_map_through() {
        let dir = tmp_dir("cache");
        write_session(
            &dir,
            "--proj--",
            "s1",
            "{\"type\":\"session\",\"id\":\"s\",\"createdAt\":1754042400000}\n\
             {\"type\":\"assistant/message\",\"seq\":1,\"time\":1754042401000,\"data\":{\"turn\":0,\"step\":0,\"message\":{\"source\":{\"model\":\"m\"}},\"usage\":{\"inputTokens\":100,\"outputTokens\":50,\"cacheReadTokens\":30,\"cacheWriteTokens\":20,\"reasoningTokens\":7}}}\n",
        );
        let events = read_deepseek(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tokens.cache_read, 30);
        assert_eq!(events[0].tokens.cache_write, 20);
        assert_eq!(events[0].tokens.reasoning, 7);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn subagent_flag_from_parent_session() {
        let dir = tmp_dir("subagent");
        write_session(
            &dir,
            "--proj--",
            "child",
            "{\"type\":\"session\",\"id\":\"child\",\"createdAt\":1,\"parentSession\":\"parent-id\"}\n\
             {\"type\":\"assistant/message\",\"seq\":1,\"time\":1754042401000,\"data\":{\"message\":{\"source\":{\"model\":\"m\"}},\"usage\":{\"inputTokens\":1,\"outputTokens\":1}}}\n",
        );
        let events = read_deepseek(&dir).events;
        assert_eq!(events.len(), 1);
        assert!(events[0].subagent);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn usage_chunks_are_ignored_only_messages_emit() {
        let dir = tmp_dir("chunks");
        write_session(
            &dir,
            "--proj--",
            "s1",
            "{\"type\":\"session\",\"id\":\"s\",\"createdAt\":1}\n\
             {\"type\":\"assistant/chunk\",\"seq\":1,\"time\":1754042401000,\"data\":{\"turn\":0,\"step\":0,\"chunk\":{\"type\":\"usage\",\"usage\":{\"inputTokens\":9999,\"outputTokens\":9999}}}}\n\
             {\"type\":\"assistant/message\",\"seq\":2,\"time\":1754042402000,\"data\":{\"turn\":0,\"step\":0,\"message\":{\"source\":{\"model\":\"m\"}},\"usage\":{\"inputTokens\":10,\"outputTokens\":5}}}\n",
        );
        let events = read_deepseek(&dir).events;
        assert_eq!(events.len(), 1, "only the finalized message emits");
        assert_eq!(events[0].tokens.input, 10);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zero_usage_messages_are_dropped() {
        let dir = tmp_dir("zero");
        write_session(
            &dir,
            "--proj--",
            "s1",
            "{\"type\":\"session\",\"id\":\"s\",\"createdAt\":1}\n\
             {\"type\":\"assistant/message\",\"seq\":1,\"time\":1754042401000,\"data\":{\"message\":{\"source\":{\"model\":\"m\"}},\"usage\":{\"inputTokens\":0,\"outputTokens\":0}}}\n",
        );
        assert!(read_deepseek(&dir).events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_required_cell_skips_the_record() {
        let dir = tmp_dir("required");
        let file = write_session(
            &dir,
            "--proj--",
            "s1",
            "{\"type\":\"session\",\"id\":\"s\",\"createdAt\":1}\n\
             {\"type\":\"assistant/message\",\"seq\":1,\"time\":1754042401000,\"data\":{\"message\":{\"source\":{\"model\":\"m\"}},\"usage\":{\"outputTokens\":5}}}\n",
        );
        let result = read_deepseek(&dir);
        assert!(result.events.is_empty());
        let loc = format!("{}:2", file.to_string_lossy());
        assert_eq!(result.skipped, vec![loc.clone()]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed deepseek record {loc}\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_zstd_produces_file_level_diagnostic() {
        let dir = tmp_dir("zstd-bad");
        let session_dir = dir.join("--proj--").join("s1");
        fs::create_dir_all(&session_dir).unwrap();
        let path = session_dir.join("session.jsonl.zstd");
        fs::write(&path, b"not a valid zstd frame").unwrap();
        let result = read_deepseek(&dir);
        assert!(result.events.is_empty());
        let file = path.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![file.clone()]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed deepseek file {file}\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_zstd_session_reads() {
        // A valid Zstandard frame of the two-line session log below, produced by
        // the reference `zstd` CLI. The reader decompresses it before scanning.
        const FRAME: &[u8] = &[
            40, 181, 47, 253, 36, 187, 29, 4, 0, 82, 9, 28, 26, 112, 105, 219, 176, 9, 189, 242,
            248, 84, 138, 110, 187, 239, 41, 171, 227, 113, 140, 86, 227, 64, 19, 64, 3, 21, 38,
            197, 123, 207, 104, 215, 142, 40, 207, 108, 11, 141, 208, 46, 97, 10, 81, 118, 199,
            247, 190, 158, 55, 108, 106, 235, 80, 92, 227, 47, 249, 37, 175, 173, 233, 104, 140,
            57, 6, 25, 200, 160, 128, 224, 228, 119, 169, 62, 158, 252, 97, 249, 248, 69, 148, 121,
            23, 160, 153, 54, 43, 102, 45, 222, 201, 247, 32, 109, 235, 42, 199, 241, 137, 231,
            147, 58, 158, 53, 197, 188, 252, 252, 38, 136, 254, 18, 5, 0, 105, 90, 160, 243, 108,
            75, 139, 41, 213, 219, 106, 154, 210, 3, 176, 9, 36, 2,
        ];
        let dir = tmp_dir("zstd-good");
        let session_dir = dir.join("--proj--").join("s1");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(session_dir.join("session.jsonl.zstd"), FRAME).unwrap();
        let events = read_deepseek(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, "z");
        assert_eq!(events[0].tokens.input, 4);
        fs::remove_dir_all(&dir).ok();
    }

    fn build_sqlite_db(path: &Path, session_rows: &[&str], event_rows: &[&str]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA application_id = {SQLITE_APPLICATION_ID};
             PRAGMA user_version = {SQLITE_SCHEMA_VERSION};
             CREATE TABLE persistence_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                store_id  TEXT NOT NULL
             );
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                version INTEGER,
                created_at INTEGER,
                cwd TEXT,
                parent_session TEXT,
                seed_length INTEGER,
                origin TEXT,
                delegation_depth INTEGER,
                agent_preset TEXT,
                incarnation TEXT,
                revision INTEGER
             );
             CREATE TABLE events (
                session_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                type TEXT NOT NULL,
                time INTEGER NOT NULL,
                data ANY NOT NULL,
                source_event_seqs ANY,
                surface_op TEXT,
                ignorable INTEGER,
                PRIMARY KEY (session_id, seq)
             );"
        ))
        .unwrap();
        for row in session_rows {
            conn.execute_batch(&format!("INSERT INTO sessions VALUES ({row});"))
                .unwrap();
        }
        for row in event_rows {
            conn.execute_batch(&format!("INSERT INTO events VALUES ({row});"))
                .unwrap();
        }
    }

    fn insert_event_blob(path: &Path, session_id: &str, seq: i64, time: i64, data: &[u8]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute(
            "INSERT INTO events (session_id, seq, type, time, data) VALUES (?1, ?2, 'assistant/message', ?3, ?4)",
            rusqlite::params![session_id, seq, time, data],
        )
        .unwrap();
    }

    fn zstd_raw_frame(payload: &[u8]) -> Vec<u8> {
        assert!(
            payload.len() < 256,
            "test helper only frames small payloads"
        );
        let mut frame = vec![0x28, 0xB5, 0x2F, 0xFD, 0x20, payload.len() as u8];
        let block_header: u32 = ((payload.len() as u32) << 3) | 1;
        frame.push((block_header & 0xFF) as u8);
        frame.push(((block_header >> 8) & 0xFF) as u8);
        frame.push(((block_header >> 16) & 0xFF) as u8);
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn sqlite_only_root_reads_events() {
        let dir = tmp_dir("sqlite-only");
        let db = dir.join("sessions.db");
        build_sqlite_db(
            &db,
            &[
                "'sess-a', 1, 1754042400000, '/home/u/proj', NULL, NULL, NULL, 0, NULL, 'i', 1",
                "'sess-b', 1, 1754046000000, '/home/u/proj', 'sess-a', NULL, 'subagent', 1, NULL, 'i', 1",
            ],
            &[
                "'sess-a', 2, 'assistant/message', 1754042402000, \
                 '{\"message\":{\"source\":{\"model\":\"deepseek-chat\"}},\"usage\":{\"inputTokens\":100,\"outputTokens\":50,\"cacheReadTokens\":30,\"cacheWriteTokens\":20,\"reasoningTokens\":7}}', \
                 NULL, NULL, NULL",
                "'sess-a', 1, 'assistant/chunk', 1754042401000, \
                 '{\"turn\":0,\"step\":0,\"chunk\":{\"type\":\"usage\",\"usage\":{\"inputTokens\":9999,\"outputTokens\":9999}}}', \
                 NULL, NULL, NULL",
                "'sess-b', 1, 'assistant/message', 1754046001000, \
                 '{\"message\":{\"source\":{\"model\":\"deepseek-reasoner\"}},\"usage\":{\"inputTokens\":200,\"outputTokens\":80,\"reasoningTokens\":30}}', \
                 NULL, NULL, NULL",
            ],
        );
        let result = read_deepseek(&dir);
        assert!(result.warnings.is_empty(), "no warnings expected");
        let mut events = result.events;
        events.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        assert_eq!(events.len(), 2, "only the two assistant/message rows emit");

        assert_eq!(events[0].session_id, "sess-a");
        assert_eq!(events[0].model, "deepseek-chat");
        assert_eq!(events[0].message_id, "sess-a:2");
        assert_eq!(events[0].timestamp, "2025-08-01T10:00:02.000Z");
        assert_eq!(events[0].tokens.input, 100);
        assert_eq!(events[0].tokens.output, 50);
        assert_eq!(events[0].tokens.cache_read, 30);
        assert_eq!(events[0].tokens.cache_write, 20);
        assert_eq!(events[0].tokens.reasoning, 7);
        assert!(!events[0].subagent);
        assert_eq!(events[0].workspace.as_deref(), Some("/home/u/proj"));

        assert_eq!(events[1].session_id, "sess-b");
        assert_eq!(events[1].model, "deepseek-reasoner");
        assert!(events[1].subagent, "parent_session tags a subagent");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn both_backends_dedupe_by_session_and_seq() {
        let dir = tmp_dir("both-backends");
        write_session(
            &dir,
            "--proj--",
            "sess-a",
            "{\"type\":\"session\",\"id\":\"sess-a\",\"createdAt\":1754042400000,\"cwd\":\"/home/u/proj\"}\n\
             {\"type\":\"assistant/message\",\"seq\":2,\"time\":1754042402000,\"data\":{\"message\":{\"source\":{\"model\":\"deepseek-chat\"}},\"usage\":{\"inputTokens\":100,\"outputTokens\":50}}}\n",
        );
        let db = dir.join("sessions.db");
        build_sqlite_db(
            &db,
            &["'sess-a', 1, 1754042400000, '/home/u/proj', NULL, NULL, NULL, 0, NULL, 'i', 1"],
            &[
                "'sess-a', 2, 'assistant/message', 1754042402000, \
                 '{\"message\":{\"source\":{\"model\":\"deepseek-chat\"}},\"usage\":{\"inputTokens\":100,\"outputTokens\":50}}', \
                 NULL, NULL, NULL",
                "'sess-a', 5, 'assistant/message', 1754042405000, \
                 '{\"message\":{\"source\":{\"model\":\"deepseek-chat\"}},\"usage\":{\"inputTokens\":11,\"outputTokens\":22}}', \
                 NULL, NULL, NULL",
            ],
        );
        let result = read_deepseek(&dir);
        assert!(result.warnings.is_empty(), "no warnings expected");
        let mut events = result.events;
        events.sort_by(|a, b| a.message_id.cmp(&b.message_id));
        assert_eq!(
            events.len(),
            2,
            "the shared sess-a:2 event is read once (JSONL wins), sess-a:5 only in sqlite"
        );
        assert!(
            events[0].message_id.ends_with("session.jsonl:1"),
            "JSONL wins the collision (its <file>:<index> id survives): {}",
            events[0].message_id
        );
        assert_eq!(events[1].message_id, "sess-a:5");
        assert_eq!(events[1].tokens.input, 11);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_db_yields_structured_diagnostic() {
        let dir = tmp_dir("sqlite-bad");
        let db = dir.join("sessions.db");
        fs::write(&db, b"this is not a valid sqlite file at all").unwrap();
        let result = read_deepseek(&dir);
        assert!(result.events.is_empty());
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![db_str.clone()]);
        assert_eq!(result.warnings.len(), 1, "one db-level diagnostic expected");
        assert!(
            result.warnings[0]
                .message
                .starts_with(&format!("unable to read deepseek database {db_str}:"))
                && result.warnings[0].message.ends_with('\n'),
            "structured db-level diagnostic: {}",
            result.warnings[0].message
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_event_row_uses_one_based_row_ordinal_label() {
        let dir = tmp_dir("sqlite-bad-row");
        let db = dir.join("sessions.db");
        build_sqlite_db(
            &db,
            &["'sess-a', 1, 1754042400000, '/home/u/proj', NULL, NULL, NULL, 0, NULL, 'i', 1"],
            &[
                "'sess-a', 3, 'assistant/message', 1754042403000, \
                 '{\"message\":{\"source\":{\"model\":\"m\"}},\"usage\":{\"outputTokens\":5}}', \
                 NULL, NULL, NULL",
                "'sess-a', 4, 'assistant/message', 1754042404000, \
                 '{\"message\":{\"source\":{\"model\":\"m\"}},\"usage\":{\"inputTokens\":1,\"outputTokens\":1}}', \
                 NULL, NULL, NULL",
            ],
        );
        let result = read_deepseek(db.parent().unwrap());
        assert_eq!(result.events.len(), 1, "only the valid row survives");
        assert_eq!(result.events[0].message_id, "sess-a:4");
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(
            result.skipped,
            vec![format!("{db_str}:1")],
            "the malformed row is the first ORDER BY result, so its one-based ordinal is 1"
        );
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed deepseek record {db_str}:1\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_deepseek_sqlite_is_ignored() {
        let dir = tmp_dir("sqlite-foreign");
        let db = dir.join("other.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE things (id TEXT); INSERT INTO things VALUES ('x');")
            .unwrap();
        drop(conn);
        let result = read_deepseek(&dir);
        assert!(result.events.is_empty(), "a foreign db emits nothing");
        assert!(
            result.warnings.is_empty(),
            "a foreign db is not a diagnostic"
        );
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zstd_blob_event_data_decompresses_and_emits() {
        let dir = tmp_dir("sqlite-zstd-good");
        let db = dir.join("sessions.db");
        build_sqlite_db(
            &db,
            &["'sess-a', 1, 1754042400000, '/home/u/proj', NULL, NULL, NULL, 0, NULL, 'i', 1"],
            &[],
        );
        let payload = br#"{"message":{"source":{"model":"deepseek-chat"}},"usage":{"inputTokens":100,"outputTokens":50,"reasoningTokens":7}}"#;
        insert_event_blob(&db, "sess-a", 2, 1754042402000, &zstd_raw_frame(payload));
        let result = read_deepseek(&dir);
        assert!(
            result.warnings.is_empty(),
            "a valid blob is not a diagnostic"
        );
        assert!(result.skipped.is_empty());
        assert_eq!(
            result.events.len(),
            1,
            "the compressed row decodes and emits"
        );
        assert_eq!(result.events[0].message_id, "sess-a:2");
        assert_eq!(result.events[0].model, "deepseek-chat");
        assert_eq!(result.events[0].tokens.input, 100);
        assert_eq!(result.events[0].tokens.output, 50);
        assert_eq!(result.events[0].tokens.reasoning, 7);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_zstd_blob_event_data_yields_row_diagnostic() {
        let dir = tmp_dir("sqlite-zstd-bad");
        let db = dir.join("sessions.db");
        build_sqlite_db(
            &db,
            &["'sess-a', 1, 1754042400000, '/home/u/proj', NULL, NULL, NULL, 0, NULL, 'i', 1"],
            &[],
        );
        let truncated = [0x28, 0xB5, 0x2F, 0xFD, 0x20, 0x40, 0x00];
        insert_event_blob(&db, "sess-a", 2, 1754042402000, &truncated);
        let result = read_deepseek(&dir);
        assert!(result.events.is_empty(), "a broken frame emits nothing");
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(
            result.skipped,
            vec![format!("{db_str}:1")],
            "the malformed blob row is the first result, one-based ordinal 1"
        );
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed deepseek record {db_str}:1\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn matching_application_id_with_wrong_schema_version_is_silently_ignored() {
        let dir = tmp_dir("sqlite-wrong-version");
        let db = dir.join("sessions.db");
        build_sqlite_db(
            &db,
            &["'sess-a', 1, 1754042400000, '/home/u/proj', NULL, NULL, NULL, 0, NULL, 'i', 1"],
            &["'sess-a', 2, 'assistant/message', 1754042402000, \
                 '{\"message\":{\"source\":{\"model\":\"deepseek-chat\"}},\"usage\":{\"inputTokens\":100,\"outputTokens\":50}}', \
                 NULL, NULL, NULL"],
        );
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};",
            SQLITE_SCHEMA_VERSION + 1
        ))
        .unwrap();
        drop(conn);
        let result = read_deepseek(&dir);
        assert!(
            result.events.is_empty(),
            "an unknown schema version is not read"
        );
        assert!(
            result.warnings.is_empty(),
            "an unknown schema version is silently ignored, not a diagnostic"
        );
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }
}

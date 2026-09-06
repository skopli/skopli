use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, file_mtime_iso, is_record, normalize_workspace,
    parse_timestamp_to_iso,
};
use super::sqlite_store::{open_readonly, query_rows, table_columns, table_exists};
use crate::types::{TokenCounts, UsageEvent};

/// The Kiro CLI reader wired into the harness registry. Kiro CLI (the
/// closed-source, backward-compatible continuation of the open-source
/// aws/amazon-q-developer-cli) persists chat to a SQLite database keyed per
/// project directory, rewritten every assistant turn. The current build stores
/// it at `~/.kiro/data.sqlite3`; a rebranded build may write
/// `data_local_dir/kiro-cli/data.sqlite3`, and an in-place upgrade from the
/// ancestor leaves the legacy `data_local_dir/amazon-q/data.sqlite3` store; all
/// are read (see `kiro_db_paths`).
///
/// The chat lives in the `conversations` `(key TEXT, value TEXT)` kv table: one
/// row per project cwd whose `value` is a serialized `ConversationState`. Its
/// `history` is an array of `{user, assistant}` entries carrying the full user
/// prompt and assistant response text verbatim. No real provider token counts
/// are ever persisted (the ancestor's `token_counter` is a runtime estimator
/// with no serialization), so this reader estimates tokens with the ancestor's
/// exact algorithm on the persisted text: the user prompt drives `input`, the
/// assistant response drives `output`. These are estimates, surfaced as ordinary
/// counts (the event model has no estimated flag); Kiro is documented as an
/// estimated harness.
///
/// A `conversations_v2` table is read defensively with the same shape when
/// present; its absence is normal and not a diagnostic. Both tables and both the
/// current and legacy databases can hold a migrated copy of one per-directory
/// conversation, so events dedupe only on evidence of equivalence: the persisted
/// `conversation_id` plus history index when the state carries one, else a
/// content fingerprint over the turn (see `dedupe_key`). A migrated copy is
/// suppressed while a distinct conversation that merely shares a project key and
/// turn index survives. When copies are equivalent, the current store wins over
/// the legacy store (path order) and `conversations_v2` wins over `conversations`
/// (table order). Cloud (`--cloud`) sessions are server-side and out of scope,
/// and the manual `/chat save` export writes to an arbitrary user-chosen path
/// that is undiscoverable from home/env, so neither is read.
pub struct KiroReader;

impl Reader for KiroReader {
    fn harness_id(&self) -> &'static str {
        "kiro"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let paths = kiro_db_paths(
            ctx.env("LOCALAPPDATA"),
            ctx.env("XDG_DATA_HOME"),
            ctx.home(),
        );
        read_kiro(&paths)
    }
}

const HARNESS: &str = "kiro";
const UNKNOWN_MODEL: &str = "kiro-unknown";
const TOKEN_TO_CHAR_RATIO: usize = 4;
/// `conversations_v2` is scanned before `conversations` so a migrated v2 copy of
/// a conversation wins over its `conversations` original under the shared
/// dedupe.
const TABLES: [&str; 2] = ["conversations_v2", "conversations"];

/// Resolve the Kiro CLI database paths, most-current first. The shipped build
/// writes `~/.kiro/data.sqlite3` (a fixed home path on every OS). A rebranded
/// build may instead write `data_local_dir/kiro-cli/data.sqlite3` (the E4
/// addendum's cross-check claim that the ancestor's `amazon-q` directory was
/// renamed to `kiro-cli`); those `kiro-cli` candidates are probed next as a
/// cheap defensive measure, since the store schema is identical. The legacy
/// ancestor build writes `data_local_dir/amazon-q/data.sqlite3`, probed last.
/// `data_local_dir` is `%LOCALAPPDATA%` on Windows, `~/Library/Application
/// Support` on macOS, and `$XDG_DATA_HOME` or `~/.local/share` on Linux; all
/// three per-OS candidates are probed for both directory names so an upgraded
/// machine is still read. Candidates are deduped first-seen-wins so a path that
/// two roots resolve to is kept once, in most-current order.
pub fn kiro_db_paths(
    localappdata: Option<&str>,
    xdg_data_home: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let push = |paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, candidate: PathBuf| {
        if seen.insert(candidate.clone()) {
            paths.push(candidate);
        }
    };

    push(
        &mut paths,
        &mut seen,
        home.join(".kiro").join("data.sqlite3"),
    );

    let localappdata = localappdata
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let macos_data = home.join("Library").join("Application Support");
    let linux_data = match xdg_data_home {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        _ => home.join(".local").join("share"),
    };

    for dir in ["kiro-cli", "amazon-q"] {
        if let Some(local) = localappdata.as_ref() {
            push(&mut paths, &mut seen, local.join(dir).join("data.sqlite3"));
        }
        push(
            &mut paths,
            &mut seen,
            macos_data.join(dir).join("data.sqlite3"),
        );
        push(
            &mut paths,
            &mut seen,
            linux_data.join(dir).join("data.sqlite3"),
        );
    }

    paths
}

/// The token estimate for a text blob, matching the ancestor's `TokenCounter`
/// exactly: the UTF-8 byte length divided by the 4-char-per-token ratio, then
/// rounded to the nearest multiple of ten via `(count + 5) / 10 * 10`. All
/// divisions are integer divisions, so text under twenty bytes estimates to 0.
fn estimate_tokens(text: &str) -> u64 {
    let count = text.len() / TOKEN_TO_CHAR_RATIO;
    ((count + 5) / 10 * 10) as u64
}

/// Extract the user prompt text from a serialized `UserMessage`. The `content`
/// field is an externally-tagged enum: `{"Prompt":{"prompt":".."}}` carries the
/// user's typed prompt; `CancelledToolUses` may carry an optional `prompt`.
/// `ToolUseResults` and any other shape carry no user-authored prompt text.
fn user_prompt(user: &Value) -> String {
    let content = match user.get("content") {
        Some(c) if is_record(c) => c,
        _ => return String::new(),
    };
    if let Some(prompt) = content
        .get("Prompt")
        .and_then(|p| p.get("prompt"))
        .and_then(as_string)
    {
        return prompt;
    }
    if let Some(prompt) = content
        .get("CancelledToolUses")
        .and_then(|c| c.get("prompt"))
        .and_then(as_string)
    {
        return prompt;
    }
    String::new()
}

/// Extract the assistant response text from a serialized `AssistantMessage`.
/// The message is an externally-tagged enum: `{"Response":{"content":".."}}` or
/// `{"ToolUse":{"content":".."}}`; both carry the assistant's natural-language
/// content in a `content` field.
fn assistant_content(assistant: &Value) -> String {
    for variant in ["Response", "ToolUse"] {
        if let Some(content) = assistant
            .get(variant)
            .and_then(|v| v.get("content"))
            .and_then(as_string)
        {
            return content;
        }
    }
    String::new()
}

/// The turn timestamp, resolved through a fallback chain. The vendor
/// `UserMessage.timestamp` (a serialized `Option<DateTime<FixedOffset>>`,
/// verified against chat-cli message.rs) is an RFC-3339 string with offsets
/// normalized to UTC and is preferred when present. When it is absent, the
/// serialized `RequestMetadata` on the `HistoryEntry` (verified against chat-cli
/// parser.rs) supplies `request_start_timestamp_ms`, then
/// `stream_end_timestamp_ms`, both Unix-millisecond epochs converted to ISO via
/// the shared `parse_timestamp_to_iso` (which routes numeric values through the
/// epoch converter). The database file mtime is the final fallback.
fn turn_timestamp(user: &Value, request_metadata: Option<&Value>, fallback: &str) -> String {
    if let Some(ts) = user.get("timestamp").and_then(parse_timestamp_to_iso) {
        return ts;
    }
    if let Some(meta) = request_metadata {
        for field in ["request_start_timestamp_ms", "stream_end_timestamp_ms"] {
            if let Some(ts) = meta.get(field).and_then(parse_timestamp_to_iso) {
                return ts;
            }
        }
    }
    fallback.to_owned()
}

/// The model id for a conversation, read from the persisted `model` string or
/// the `model_info.model_id`, else the unknown-model sentinel.
fn conversation_model(state: &Value) -> String {
    state
        .get("model")
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .or_else(|| {
            state
                .get("model_info")
                .and_then(|mi| mi.get("model_id"))
                .and_then(as_string)
                .filter(|m| !m.trim().is_empty())
        })
        .unwrap_or_else(|| UNKNOWN_MODEL.to_owned())
}

/// The persisted stable conversation identifier for one row. The vendor
/// `ConversationState` serializes a `conversation_id: String`
/// (E4-kiro-schema.md), which is the durable identity of a conversation: a
/// migration copy of a conversation into the legacy store keeps the same
/// `conversation_id`, while a reset or independently continued history gets a
/// fresh one. The JSON value is the vendor-serialized identity and always wins
/// when present. Some real `conversations_v2` tables also store the id in a
/// dedicated `conversation_id` column; when the JSON state omits its own id, a
/// non-empty column value (`column_id`) is used instead so cross-store dedupe
/// still keys on the durable identity.
fn conversation_id(state: &Value, column_id: Option<&str>) -> Option<String> {
    state
        .get("conversation_id")
        .and_then(as_string)
        .filter(|id| !id.trim().is_empty())
        .or_else(|| {
            column_id
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        })
}

/// Append a variable-length field to a dedupe key as `{byte_len}:{field}`. The
/// byte-length prefix makes the concatenation injective: because `field` is
/// consumed by its declared length, no field value can borrow bytes from a
/// neighbour, so two distinct tuples can never encode to the same string even
/// when a field contains the separator or a colon.
fn push_field(buf: &mut String, field: &str) {
    use std::fmt::Write as _;
    let _ = write!(buf, "{}:{}", field.len(), field);
}

/// The cross-store dedupe key for one turn. When the conversation carries a
/// stable `conversation_id`, two turns are the same turn iff they share that id
/// and the history index, so the key is `cid` plus the length-prefixed id and
/// index. Without a persisted id we cannot establish identity by reference, so
/// we fall back to a content fingerprint over the equivalence evidence (project
/// key, index, turn timestamp, prompt, response, model): two id-less turns whose
/// entire encoded tuple matches are indistinguishable from a migrated copy and
/// are collapsed, while any difference in a field makes the turn survive. Fields
/// are length-prefixed (see `push_field`) so the encoding is injective even
/// though prompt and response are arbitrary text that may contain the separator.
/// The `cid` and `fp` prefixes differ before the first field, so the two schemes
/// cannot collide with each other.
fn dedupe_key(
    conversation_id: Option<&str>,
    key: &str,
    index: usize,
    timestamp: &str,
    prompt: &str,
    response: &str,
    model: &str,
) -> String {
    let index = index.to_string();
    match conversation_id {
        Some(id) => {
            let mut buf = String::from("cid\u{1f}");
            push_field(&mut buf, id);
            push_field(&mut buf, &index);
            buf
        }
        None => {
            let mut buf = String::from("fp\u{1f}");
            push_field(&mut buf, key);
            push_field(&mut buf, &index);
            push_field(&mut buf, timestamp);
            push_field(&mut buf, prompt);
            push_field(&mut buf, response);
            push_field(&mut buf, model);
            buf
        }
    }
}

/// Emit one event per completed history turn of one `ConversationState`. The
/// `key` is the project cwd (the kv row key) and is both the session id and the
/// normalized workspace. `next_message` (the pending, unsent user turn with no
/// assistant reply) is not part of `history` and is not emitted. Each turn's
/// cross-store dedupe key (see `dedupe_key`) is recorded in `seen`; a turn whose
/// equivalence-evidence is already present from an earlier-precedence table or
/// database is skipped so a migrated copy of a conversation never double-counts,
/// while a distinct conversation sharing the same project key and index (a reset
/// or independently continued history) survives.
fn read_conversation(
    key: &str,
    state: &Value,
    column_id: Option<&str>,
    fallback_ts: &str,
    seen: &mut HashSet<String>,
    events: &mut Vec<UsageEvent>,
) {
    let history = match state.get("history").and_then(Value::as_array) {
        Some(h) => h,
        None => return,
    };
    let model = conversation_model(state);
    let cid = conversation_id(state, column_id);
    let workspace = normalize_workspace(&Value::String(key.to_owned()));
    for (index, entry) in history.iter().enumerate() {
        if !is_record(entry) {
            continue;
        }
        let user = match entry.get("user") {
            Some(u) if is_record(u) => u,
            _ => continue,
        };
        let assistant = match entry.get("assistant") {
            Some(a) if is_record(a) => a,
            _ => continue,
        };
        let prompt = user_prompt(user);
        let response = assistant_content(assistant);
        let input = estimate_tokens(&prompt);
        let output = estimate_tokens(&response);
        if input == 0 && output == 0 {
            continue;
        }
        let request_metadata = entry.get("request_metadata").filter(|m| is_record(m));
        let timestamp = turn_timestamp(user, request_metadata, fallback_ts);
        let seen_key = dedupe_key(
            cid.as_deref(),
            key,
            index,
            &timestamp,
            &prompt,
            &response,
            &model,
        );
        if seen.contains(&seen_key) {
            continue;
        }
        seen.insert(seen_key);
        events.push(UsageEvent {
            harness: HARNESS.to_owned(),
            timestamp,
            session_id: key.to_owned(),
            message_id: format!("{key}:{index}"),
            turn: true,
            subagent: false,
            model: model.clone(),
            tokens: TokenCounts {
                input,
                output,
                cache_read: 0,
                cache_write: 0,
                cache_write1h: None,
                reasoning: 0,
            },
            calls: None,
            cost_usd: None,
            workspace: workspace.clone(),
            title: None,
        });
    }
}

/// Read one Kiro CLI SQLite database into usage events. Each present chat table
/// (`conversations_v2` first, then `conversations`) is read as a `(key, value)`
/// kv store: `key` is the project cwd, `value` is a serialized
/// `ConversationState`. A row whose `value` is absent or not valid JSON is
/// reported as malformed against a `{db}:{table}:{ordinal}` identity, where
/// `ordinal` is its one-based row position within that table's deterministic
/// `ORDER BY key` scan (the table scopes the identity so the same ordinal in two
/// tables never collides). `seen` carries the shared turn-dedupe identities
/// across tables and databases. When the scanned table has a `conversation_id`
/// column (probed via `table_columns`), it is selected and passed as the
/// column-level id fallback; the JSON state's own `conversation_id` still wins
/// when present (see `conversation_id`).
fn read_db(
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
    let fallback_ts = file_mtime_iso(db_str);

    for table in TABLES {
        if !table_exists(&conn, table) {
            continue;
        }
        let has_cid_column = table_columns(&conn, table).contains("conversation_id");
        let query = if has_cid_column {
            format!("SELECT key, value, conversation_id FROM {table} ORDER BY key")
        } else {
            format!("SELECT key, value FROM {table} ORDER BY key")
        };
        let rows = match query_rows(&conn, &query) {
            Ok(r) => r,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read {HARNESS} database {db_str}: {e}\n"),
                });
                skipped.push(db_str.to_owned());
                continue;
            }
        };
        for (position, row) in rows.iter().enumerate() {
            let ordinal = position + 1;
            let label = format!("{db_str}:{table}:{ordinal}");
            let key = match row.get("key").and_then(as_string) {
                Some(k) => k,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed {HARNESS} record {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            let value = match row.get("value").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed {HARNESS} record {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            let state: Value = match serde_json::from_str(value) {
                Ok(v) if is_record(&v) => v,
                _ => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed {HARNESS} record {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            let column_id = if has_cid_column {
                row.get("conversation_id").and_then(as_string)
            } else {
                None
            };
            read_conversation(
                &key,
                &state,
                column_id.as_deref(),
                &fallback_ts,
                seen,
                &mut events,
            );
        }
    }

    events
}

/// Read all Kiro CLI usage events reachable from the resolved database paths.
/// Each existing database is read; a path that does not exist is skipped
/// silently (absence is normal, not a diagnostic). One event is emitted per
/// completed history turn, with tokens estimated by the ancestor's algorithm
/// from the persisted message text. `db_paths` is in precedence order (current
/// store before legacy), and a shared `seen` set dedupes the same conversation
/// turn across databases and tables so a migration never double-counts.
pub fn read_kiro(db_paths: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for path in db_paths {
        if !path.is_file() {
            continue;
        }
        let db_str = path.to_string_lossy().into_owned();
        events.extend(read_db(&db_str, &mut seen, &mut skipped, &mut warnings));
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
    fn default_paths_prefer_dot_kiro_then_kiro_cli_then_legacy() {
        let paths = kiro_db_paths(None, None, &home());
        assert_eq!(paths[0], home().join(".kiro").join("data.sqlite3"));
        let kiro_cli = home()
            .join(".local")
            .join("share")
            .join("kiro-cli")
            .join("data.sqlite3");
        let amazon_q = home()
            .join(".local")
            .join("share")
            .join("amazon-q")
            .join("data.sqlite3");
        let kiro_cli_pos = paths.iter().position(|p| p == &kiro_cli).unwrap();
        let amazon_q_pos = paths.iter().position(|p| p == &amazon_q).unwrap();
        assert!(
            kiro_cli_pos < amazon_q_pos,
            "the kiro-cli candidate is probed before the amazon-q legacy candidate"
        );
    }

    #[test]
    fn localappdata_adds_windows_kiro_cli_and_legacy_roots() {
        let paths = kiro_db_paths(Some("C:\\Users\\u\\AppData\\Local"), None, &home());
        assert!(
            paths.contains(
                &PathBuf::from("C:\\Users\\u\\AppData\\Local")
                    .join("kiro-cli")
                    .join("data.sqlite3")
            )
        );
        assert!(
            paths.contains(
                &PathBuf::from("C:\\Users\\u\\AppData\\Local")
                    .join("amazon-q")
                    .join("data.sqlite3")
            )
        );
    }

    #[test]
    fn xdg_data_home_overrides_linux_kiro_cli_and_legacy_roots() {
        let paths = kiro_db_paths(None, Some("/xdg/data"), &home());
        assert!(
            paths.contains(
                &PathBuf::from("/xdg/data")
                    .join("kiro-cli")
                    .join("data.sqlite3")
            )
        );
        assert!(
            paths.contains(
                &PathBuf::from("/xdg/data")
                    .join("amazon-q")
                    .join("data.sqlite3")
            )
        );
        assert!(
            !paths.contains(
                &home()
                    .join(".local")
                    .join("share")
                    .join("amazon-q")
                    .join("data.sqlite3")
            )
        );
    }

    #[test]
    fn candidates_are_deduped_first_seen_wins() {
        let paths = kiro_db_paths(None, None, &home());
        let mut unique = paths.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            paths.len(),
            unique.len(),
            "no candidate path appears twice across the probed roots"
        );
    }

    #[test]
    fn upstream_byte_len_over_four_rounded_to_nearest_ten() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(
            estimate_tokens(&"x".repeat(19)),
            0,
            "19 bytes -> count 4 -> rounds to 0"
        );
        assert_eq!(
            estimate_tokens(&"x".repeat(20)),
            10,
            "20 bytes -> count 5 -> rounds to 10"
        );
        assert_eq!(
            estimate_tokens(&"x".repeat(59)),
            10,
            "59 bytes -> count 14 -> rounds to 10"
        );
        assert_eq!(
            estimate_tokens(&"x".repeat(60)),
            20,
            "60 bytes -> count 15 -> rounds to 20"
        );
        assert_eq!(
            estimate_tokens(&"x".repeat(100)),
            30,
            "100 bytes -> count 25 -> rounds to 30"
        );
    }

    #[test]
    fn estimate_uses_utf8_byte_length_not_char_count() {
        let ascii = "x".repeat(40);
        let multibyte = "é".repeat(20);
        assert_eq!(ascii.chars().count(), 40);
        assert_eq!(multibyte.chars().count(), 20);
        assert_eq!(multibyte.len(), 40, "each 'é' is two UTF-8 bytes");
        assert_eq!(
            estimate_tokens(&multibyte),
            estimate_tokens(&ascii),
            "equal byte length estimates equally despite half the chars"
        );
        assert_eq!(estimate_tokens(&multibyte), 10);
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-kiro-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn build_db(path: &Path, table: &str, rows: &[(&str, &str)]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE {table} (key TEXT PRIMARY KEY, value TEXT);"
        ))
        .unwrap();
        for (key, value) in rows {
            conn.execute(
                &format!("INSERT INTO {table} (key, value) VALUES (?1, ?2)"),
                rusqlite::params![key, value],
            )
            .unwrap();
        }
    }

    fn state_json(model: &str, turns: &[(&str, &str, Option<&str>)]) -> String {
        let history: Vec<Value> = turns
            .iter()
            .map(|(prompt, response, ts)| {
                let mut user = serde_json::json!({
                    "content": { "Prompt": { "prompt": prompt } }
                });
                if let Some(ts) = ts {
                    user["timestamp"] = Value::String((*ts).to_owned());
                } else {
                    user["timestamp"] = Value::Null;
                }
                serde_json::json!({
                    "user": user,
                    "assistant": { "Response": { "message_id": null, "content": response } }
                })
            })
            .collect();
        serde_json::json!({ "model": model, "history": history }).to_string()
    }

    fn state_json_cid(cid: &str, model: &str, turns: &[(&str, &str, Option<&str>)]) -> String {
        let mut state: Value = serde_json::from_str(&state_json(model, turns)).unwrap();
        state["conversation_id"] = Value::String(cid.to_owned());
        state.to_string()
    }

    #[test]
    fn empty_paths_yield_no_events_then_read_after_db_appears() {
        let dir = tmp_dir("presence");
        let db = dir.join("data.sqlite3");
        assert!(read_kiro(std::slice::from_ref(&db)).events.is_empty());
        let prompt = "p".repeat(40);
        let response = "r".repeat(40);
        build_db(
            &db,
            "conversations",
            &[(
                "/home/u/proj",
                &state_json(
                    "claude-sonnet",
                    &[(
                        prompt.as_str(),
                        response.as_str(),
                        Some("2026-08-01T10:00:00Z"),
                    )],
                ),
            )],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, "/home/u/proj");
        assert_eq!(events[0].model, "claude-sonnet");
        assert_eq!(events[0].timestamp, "2026-08-01T10:00:00.000Z");
        assert_eq!(events[0].tokens.input, 10);
        assert_eq!(events[0].tokens.output, 10);
        assert!(events[0].turn);
        assert!(!events[0].subagent);
        assert_eq!(events[0].workspace.as_deref(), Some("/home/u/proj"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn one_event_per_history_turn() {
        let dir = tmp_dir("turns");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        build_db(
            &db,
            "conversations",
            &[(
                "/home/u/proj",
                &state_json(
                    "m",
                    &[
                        (text.as_str(), text.as_str(), None),
                        (text.as_str(), text.as_str(), None),
                    ],
                ),
            )],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].message_id, "/home/u/proj:0");
        assert_eq!(events[1].message_id, "/home/u/proj:1");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_timestamp_falls_back_to_file_mtime() {
        let dir = tmp_dir("fallback-ts");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        build_db(
            &db,
            "conversations",
            &[(
                "/home/u/proj",
                &state_json("m", &[(text.as_str(), text.as_str(), None)]),
            )],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(events.len(), 1);
        assert!(events[0].timestamp.ends_with('Z'));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zero_estimate_turns_are_dropped() {
        let dir = tmp_dir("zero");
        let db = dir.join("data.sqlite3");
        build_db(
            &db,
            "conversations",
            &[("/home/u/proj", &state_json("m", &[("hi", "ok", None)]))],
        );
        assert!(
            read_kiro(&[db]).events.is_empty(),
            "sub-20-byte turns estimate to zero"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn conversations_v2_is_read_when_present() {
        let dir = tmp_dir("v2");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        build_db(
            &db,
            "conversations_v2",
            &[(
                "/home/u/proj",
                &state_json("m", &[(text.as_str(), text.as_str(), None)]),
            )],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(events.len(), 1, "the v2 table is read defensively");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tool_use_assistant_content_is_estimated() {
        let dir = tmp_dir("tooluse");
        let db = dir.join("data.sqlite3");
        let prompt = "p".repeat(40);
        let content = "c".repeat(60);
        let state = serde_json::json!({
            "model": "m",
            "history": [{
                "user": { "content": { "Prompt": { "prompt": prompt } }, "timestamp": null },
                "assistant": { "ToolUse": { "message_id": null, "content": content, "tool_uses": [] } }
            }]
        })
        .to_string();
        build_db(&db, "conversations", &[("/home/u/proj", &state)]);
        let events = read_kiro(&[db]).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tokens.input, 10, "40 bytes rounds to 10");
        assert_eq!(events[0].tokens.output, 20, "60 bytes rounds to 20");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_value_uses_one_based_row_ordinal_scoped_by_table() {
        let dir = tmp_dir("malformed-row");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        build_db(
            &db,
            "conversations",
            &[
                ("/aaa", "not valid json at all"),
                (
                    "/bbb",
                    &state_json("m", &[(text.as_str(), text.as_str(), None)]),
                ),
            ],
        );
        let result = read_kiro(std::slice::from_ref(&db));
        assert_eq!(result.events.len(), 1, "only the valid row survives");
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(
            result.skipped,
            vec![format!("{db_str}:conversations:1")],
            "the malformed row is the first ORDER BY key result, one-based ordinal 1, table-scoped"
        );
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed kiro record {db_str}:conversations:1\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    fn build_db_two_tables(path: &Path, v2_rows: &[(&str, &str)], v1_rows: &[(&str, &str)]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        for table in ["conversations_v2", "conversations"] {
            conn.execute_batch(&format!(
                "CREATE TABLE {table} (key TEXT PRIMARY KEY, value TEXT);"
            ))
            .unwrap();
        }
        for (table, rows) in [("conversations_v2", v2_rows), ("conversations", v1_rows)] {
            for (key, value) in rows {
                conn.execute(
                    &format!("INSERT INTO {table} (key, value) VALUES (?1, ?2)"),
                    rusqlite::params![key, value],
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn two_table_malformed_rows_have_distinct_table_scoped_identities() {
        let dir = tmp_dir("two-table-malformed");
        let db = dir.join("data.sqlite3");
        build_db_two_tables(
            &db,
            &[("/aaa", "not valid json in v2")],
            &[("/aaa", "not valid json in v1")],
        );
        let result = read_kiro(std::slice::from_ref(&db));
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(
            result.skipped,
            vec![
                format!("{db_str}:conversations_v2:1"),
                format!("{db_str}:conversations:1"),
            ],
            "the first malformed row of each table keeps a distinct table-scoped identity"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrated_conversation_id_copy_in_both_tables_is_not_double_counted() {
        let dir = tmp_dir("dup-tables-cid");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        let state = state_json_cid("conv-abc", "m", &[(text.as_str(), text.as_str(), None)]);
        build_db_two_tables(
            &db,
            &[("/home/u/proj", state.as_str())],
            &[("/home/u/proj", state.as_str())],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(
            events.len(),
            1,
            "a copy sharing conversation_id and index is a migration duplicate"
        );
        assert_eq!(events[0].message_id, "/home/u/proj:0");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exact_copy_without_conversation_id_in_both_tables_is_not_double_counted() {
        let dir = tmp_dir("dup-tables-fp");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        let state = state_json(
            "m",
            &[(text.as_str(), text.as_str(), Some("2026-08-01T10:00:00Z"))],
        );
        build_db_two_tables(
            &db,
            &[("/home/u/proj", state.as_str())],
            &[("/home/u/proj", state.as_str())],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(
            events.len(),
            1,
            "an id-less byte-for-byte copy is suppressed by the content fingerprint"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn distinct_conversations_sharing_key_and_index_survive_across_tables() {
        let dir = tmp_dir("distinct-tables");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        let v2 = state_json_cid("conv-v2", "m", &[(text.as_str(), text.as_str(), None)]);
        let v1 = state_json_cid("conv-v1", "m", &[(text.as_str(), text.as_str(), None)]);
        build_db_two_tables(
            &db,
            &[("/home/u/proj", v2.as_str())],
            &[("/home/u/proj", v1.as_str())],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(
            events.len(),
            2,
            "different conversation_ids at the same key and index are not duplicates"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn different_content_at_same_key_and_index_survives_across_tables() {
        let dir = tmp_dir("distinct-tables-fp");
        let db = dir.join("data.sqlite3");
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let v2 = state_json(
            "m",
            &[(a.as_str(), a.as_str(), Some("2026-08-01T10:00:00Z"))],
        );
        let v1 = state_json(
            "m",
            &[(b.as_str(), b.as_str(), Some("2026-08-01T10:00:00Z"))],
        );
        build_db_two_tables(
            &db,
            &[("/home/u/proj", v2.as_str())],
            &[("/home/u/proj", v1.as_str())],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(
            events.len(),
            2,
            "id-less turns with different content are distinct, not migration copies"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn id_less_turns_differing_only_by_separator_placement_both_survive() {
        let dir = tmp_dir("distinct-tables-sep");
        let db = dir.join("data.sqlite3");
        let x = "x".repeat(20);
        let y = "y".repeat(20);
        let z = "z".repeat(20);
        let prompt_a = format!("{x}\u{1f}{y}");
        let response_a = z.clone();
        let prompt_b = x.clone();
        let response_b = format!("{y}\u{1f}{z}");
        assert_eq!(
            format!("{prompt_a}\u{1f}{response_a}"),
            format!("{prompt_b}\u{1f}{response_b}"),
            "the two turns collide under a raw separator-joined encoding"
        );
        let v2 = state_json(
            "m",
            &[(
                prompt_a.as_str(),
                response_a.as_str(),
                Some("2026-08-01T10:00:00Z"),
            )],
        );
        let v1 = state_json(
            "m",
            &[(
                prompt_b.as_str(),
                response_b.as_str(),
                Some("2026-08-01T10:00:00Z"),
            )],
        );
        build_db_two_tables(
            &db,
            &[("/home/u/proj", v2.as_str())],
            &[("/home/u/proj", v1.as_str())],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(
            events.len(),
            2,
            "distinct turns differing only by separator placement must not collide"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrated_conversation_id_copy_in_legacy_db_is_not_double_counted() {
        let dir = tmp_dir("dup-dbs-cid");
        let current = dir.join("current.sqlite3");
        let legacy = dir.join("legacy.sqlite3");
        let text = "z".repeat(40);
        let state = state_json_cid("conv-xyz", "m", &[(text.as_str(), text.as_str(), None)]);
        build_db(
            &current,
            "conversations",
            &[("/home/u/proj", state.as_str())],
        );
        build_db(
            &legacy,
            "conversations",
            &[("/home/u/proj", state.as_str())],
        );
        let events = read_kiro(&[current, legacy]).events;
        assert_eq!(
            events.len(),
            1,
            "the legacy copy sharing conversation_id is deduped against the current store"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn independently_continued_legacy_history_is_not_dropped() {
        let dir = tmp_dir("distinct-dbs");
        let current = dir.join("current.sqlite3");
        let legacy = dir.join("legacy.sqlite3");
        let text = "z".repeat(40);
        let current_state = state_json_cid(
            "conv-current",
            "current-model",
            &[(text.as_str(), text.as_str(), None)],
        );
        let legacy_state = state_json_cid(
            "conv-legacy",
            "legacy-model",
            &[(text.as_str(), text.as_str(), None)],
        );
        build_db(
            &current,
            "conversations",
            &[("/home/u/proj", current_state.as_str())],
        );
        build_db(
            &legacy,
            "conversations",
            &[("/home/u/proj", legacy_state.as_str())],
        );
        let events = read_kiro(&[current, legacy]).events;
        assert_eq!(
            events.len(),
            2,
            "a distinct legacy conversation at the same key and index is not a migration copy"
        );
        let mut models: Vec<&str> = events.iter().map(|e| e.model.as_str()).collect();
        models.sort_unstable();
        assert_eq!(models, vec!["current-model", "legacy-model"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_history_without_conversation_id_survives_across_dbs() {
        let dir = tmp_dir("reset-dbs-fp");
        let current = dir.join("current.sqlite3");
        let legacy = dir.join("legacy.sqlite3");
        let text = "z".repeat(40);
        let current_state = state_json(
            "m",
            &[(text.as_str(), text.as_str(), Some("2026-08-02T09:00:00Z"))],
        );
        let legacy_state = state_json(
            "m",
            &[(text.as_str(), text.as_str(), Some("2026-08-01T08:00:00Z"))],
        );
        build_db(
            &current,
            "conversations",
            &[("/home/u/proj", current_state.as_str())],
        );
        build_db(
            &legacy,
            "conversations",
            &[("/home/u/proj", legacy_state.as_str())],
        );
        let events = read_kiro(&[current, legacy]).events;
        assert_eq!(
            events.len(),
            2,
            "id-less turns differing only by timestamp are distinct across databases"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_database_yields_structured_diagnostic() {
        let dir = tmp_dir("bad-db");
        let db = dir.join("data.sqlite3");
        fs::write(&db, b"this is not a valid sqlite file").unwrap();
        let result = read_kiro(std::slice::from_ref(&db));
        assert!(result.events.is_empty());
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![db_str.clone()]);
        assert!(
            result.warnings[0]
                .message
                .starts_with(&format!("unable to read kiro database {db_str}:"))
                && result.warnings[0].message.ends_with('\n')
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn database_without_chat_table_is_silent() {
        let dir = tmp_dir("no-table");
        let db = dir.join("data.sqlite3");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE state (key TEXT, value TEXT);")
            .unwrap();
        drop(conn);
        let result = read_kiro(&[db]);
        assert!(result.events.is_empty());
        assert!(
            result.warnings.is_empty(),
            "an absent chat table is not a diagnostic"
        );
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    fn build_db_three_column(path: &Path, table: &str, rows: &[(&str, &str, Option<&str>)]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE {table} (key TEXT PRIMARY KEY, conversation_id TEXT, value TEXT);"
        ))
        .unwrap();
        for (key, value, cid) in rows {
            conn.execute(
                &format!("INSERT INTO {table} (key, conversation_id, value) VALUES (?1, ?2, ?3)"),
                rusqlite::params![key, cid, value],
            )
            .unwrap();
        }
    }

    #[test]
    fn conversation_id_column_dedupes_by_cid_across_tables_and_dbs() {
        let dir = tmp_dir("cid-column-dedupe");
        let current = dir.join("current.sqlite3");
        let legacy = dir.join("legacy.sqlite3");
        let text = "z".repeat(40);
        let state = state_json("m", &[(text.as_str(), text.as_str(), None)]);
        build_db_three_column(
            &current,
            "conversations_v2",
            &[("/home/u/proj", state.as_str(), Some("conv-col"))],
        );
        build_db_three_column(
            &legacy,
            "conversations_v2",
            &[("/home/u/proj", state.as_str(), Some("conv-col"))],
        );
        let events = read_kiro(&[current, legacy]).events;
        assert_eq!(
            events.len(),
            1,
            "id-only-in-column copies sharing the column id dedupe across tables and dbs"
        );
        assert_eq!(events[0].message_id, "/home/u/proj:0");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn json_conversation_id_wins_over_differing_column_id() {
        let dir = tmp_dir("cid-json-wins");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        let state = state_json_cid("conv-json", "m", &[(text.as_str(), text.as_str(), None)]);
        build_db_three_column(
            &db,
            "conversations_v2",
            &[
                ("/aaa", state.as_str(), Some("conv-column-a")),
                ("/bbb", state.as_str(), Some("conv-column-b")),
            ],
        );
        let events = read_kiro(&[db]).events;
        assert_eq!(
            events.len(),
            1,
            "the JSON conversation_id wins over the column id, so both rows share the same durable identity and dedupe to one"
        );
        fs::remove_dir_all(&dir).ok();
    }

    fn state_json_request_metadata(
        model: &str,
        prompt: &str,
        response: &str,
        meta: Value,
    ) -> String {
        let entry = serde_json::json!({
            "user": { "content": { "Prompt": { "prompt": prompt } }, "timestamp": null },
            "assistant": { "Response": { "message_id": null, "content": response } },
            "request_metadata": meta
        });
        serde_json::json!({ "model": model, "history": [entry] }).to_string()
    }

    #[test]
    fn timestamp_falls_back_to_request_metadata_start() {
        let dir = tmp_dir("rm-timestamp");
        let db = dir.join("data.sqlite3");
        let text = "z".repeat(40);
        let meta = serde_json::json!({
            "request_start_timestamp_ms": 1_754_042_400_000i64,
            "stream_end_timestamp_ms": 1_754_042_460_000i64
        });
        let state = state_json_request_metadata("m", text.as_str(), text.as_str(), meta);
        build_db(&db, "conversations", &[("/home/u/proj", &state)]);
        let events = read_kiro(&[db]).events;
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].timestamp, "2025-08-01T10:00:00.000Z",
            "the request_metadata start ms epoch is the timestamp when the user turn omits its own"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn next_message_pending_turn_is_not_emitted() {
        let dir = tmp_dir("next-message");
        let db = dir.join("data.sqlite3");
        let state = serde_json::json!({
            "model": "m",
            "next_message": { "content": { "Prompt": { "prompt": "pending unsent prompt here" } }, "timestamp": null },
            "history": []
        })
        .to_string();
        build_db(&db, "conversations", &[("/home/u/proj", &state)]);
        assert!(
            read_kiro(&[db]).events.is_empty(),
            "only completed history turns emit; the pending next_message does not"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

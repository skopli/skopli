use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{EpochUnit, ReaderResult, ReaderWarning, as_string, epoch_to_iso_unit};
use super::sqlite_store::{open_readonly, query_rows, table_exists};
use crate::types::{TokenCounts, UsageEvent};

// Cherry Studio's `ai_usage_record.record_kind` distinguishes two row shapes:
// `invocation` rows are per-request usage recorded going forward, and
// `legacy-aggregate` rows are the one-time v1 -> v2 backfill of pre-existing
// assistant-message usage that predates per-invocation tracking. Primary source
// (CherryHQ/cherry-studio) establishes these two are disjoint by construction:
// the v2 migrator reads historical messages from the old message tables (not
// from ai_usage_record), writes one aggregate per message keyed by a
// `legacy:<kind>:<id>` requestId, while invocation rows use the live provider
// requestId; requestId is unique and inserts are onConflictDoNothing, so the two
// namespaces cannot collide and never describe the same usage. Aggregates carry
// pre-v2 message timestamps and invocations carry post-v2 completion times.
// (Upstream: AiUsageRecordMigrator, the aiUsageRecord schema, and
// AiUsageRecordService in CherryHQ/cherry-studio.) The rule shipped here therefore
// emits one event per row for BOTH kinds; each row is deduped by its own `id`,
// so no double counting occurs. Rows with any other record_kind (or a NULL kind)
// are treated as invocations.

/// The Cherry Studio reader wired into the harness registry. It reads one event
/// per `ai_usage_record` row from the single `cherrystudio.sqlite` database,
/// which Cherry Studio opens in WAL mode; the shared read-only open honors the
/// WAL sidecars.
pub struct CherryStudioReader;

impl Reader for CherryStudioReader {
    fn harness_id(&self) -> &'static str {
        "cherrystudio"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let paths = cherrystudio_db_paths(ctx.env("APPDATA"), ctx.home());
        read_cherrystudio(&paths)
    }
}

/// Resolve the Cherry Studio database paths. The DB lives under the Electron
/// `userData/Data` directory for productName "CherryStudio": Windows
/// `%APPDATA%\CherryStudio\Data\cherrystudio.sqlite`, macOS `~/Library/
/// Application Support/CherryStudio/Data/cherrystudio.sqlite`, Linux
/// `~/.config/CherryStudio/Data/cherrystudio.sqlite`.
pub fn cherrystudio_db_paths(appdata: Option<&str>, home: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        home.join(".config")
            .join("CherryStudio")
            .join("Data")
            .join("cherrystudio.sqlite"),
        home.join("Library")
            .join("Application Support")
            .join("CherryStudio")
            .join("Data")
            .join("cherrystudio.sqlite"),
    ];
    if let Some(app) = appdata
        && !app.trim().is_empty()
    {
        paths.push(
            PathBuf::from(app.trim())
                .join("CherryStudio")
                .join("Data")
                .join("cherrystudio.sqlite"),
        );
    }
    paths
}

/// Decode a token cell to a count. NULL, an absent column, or an empty string
/// are legitimately absent and decode to 0. A populated cell that is not a
/// non-negative integer count (malformed text, a negative or fractional value,
/// or a non-numeric JSON type) yields `None` so the caller skips the whole row
/// with a diagnostic. Cherry Studio's own schema enforces every token column as
/// `NULL OR (>= 0 AND typeof = 'integer')` (upstream checks
/// `ai_usage_record_nonnegative_check` / `ai_usage_record_integer_check`), so
/// any populated invalid cell means schema drift or external corruption, not
/// normal data.
fn numeric_field(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) if s.trim().is_empty() => Some(0),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// The `String(row["id"] ?? "")` fallback for a non-string id.
fn id_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Compose `provider/model`. Falls back to whichever of model_id/model_name is
/// present; an entirely absent model yields an empty string (Skopli's
/// unknown-model convention).
fn compose_model(row: &Value) -> String {
    let model = row
        .get("model_id")
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .or_else(|| {
            row.get("model_name")
                .and_then(as_string)
                .filter(|m| !m.trim().is_empty())
        });
    let model = match model {
        Some(m) => m.trim().to_owned(),
        None => return String::new(),
    };
    match row.get("provider_id").and_then(as_string) {
        Some(p) if !p.trim().is_empty() => format!("{}/{model}", p.trim()),
        _ => model,
    }
}

/// Read all Cherry Studio usage events.
pub fn read_cherrystudio(db_paths: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for db_path in db_paths {
        if !db_path.exists() {
            continue;
        }
        let db_str = db_path.to_string_lossy().into_owned();
        let conn = match open_readonly(&db_str) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read cherrystudio database {db_str}: {e}\n"),
                });
                skipped.push(db_str);
                continue;
            }
        };
        if !table_exists(&conn, "ai_usage_record") {
            continue;
        }
        let rows = match query_rows(
            &conn,
            "SELECT id, input_tokens, output_tokens, reasoning_tokens, \
             cache_read_tokens, cache_write_tokens, model_id, model_name, \
             provider_id, created_at, message_id, message_kind, record_kind \
             FROM ai_usage_record",
        ) {
            Ok(r) => r,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read cherrystudio database {db_str}: {e}\n"),
                });
                skipped.push(db_str);
                continue;
            }
        };
        for row in &rows {
            if !row.is_object() {
                continue;
            }
            let record_id = row
                .get("id")
                .and_then(as_string)
                .unwrap_or_else(|| id_string(row.get("id")));
            if record_id.is_empty() {
                continue;
            }
            let dedup_key = format!("{db_str}:{record_id}");
            if seen.contains(&dedup_key) {
                continue;
            }
            let label = dedup_key.clone();

            let counts = [
                numeric_field(row.get("input_tokens")),
                numeric_field(row.get("output_tokens")),
                numeric_field(row.get("reasoning_tokens")),
                numeric_field(row.get("cache_read_tokens")),
                numeric_field(row.get("cache_write_tokens")),
            ];
            let [input, output, reasoning, cache_read, cache_write] = match counts {
                [Some(i), Some(o), Some(r), Some(cr), Some(cw)] => [i, o, r, cr, cw],
                _ => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed cherrystudio record {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            if input == 0 && output == 0 && reasoning == 0 && cache_read == 0 && cache_write == 0 {
                continue;
            }

            // created_at is INTEGER epoch milliseconds. Cherry Studio's schema
            // defaults it to `Date.now()` and its own read path buckets by
            // `date(created_at / 1000, 'unixepoch')`, both confirming ms.
            let timestamp = match row
                .get("created_at")
                .and_then(|v| epoch_to_iso_unit(v, EpochUnit::Ms))
            {
                Some(t) => t,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed cherrystudio record {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };

            let message_id = row
                .get("message_id")
                .and_then(as_string)
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| record_id.clone());

            seen.insert(dedup_key);
            events.push(UsageEvent {
                harness: "cherrystudio".to_owned(),
                timestamp,
                session_id: record_id,
                message_id,
                turn: false,
                subagent: false,
                model: compose_model(row),
                tokens: TokenCounts {
                    input,
                    output,
                    cache_read,
                    cache_write,
                    cache_write1h: None,
                    reasoning,
                },
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            });
        }
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
    use rusqlite::Connection;
    use std::fs;

    fn home() -> PathBuf {
        PathBuf::from("/home/u")
    }

    #[test]
    fn appdata_path_is_appended_when_present() {
        let paths = cherrystudio_db_paths(Some("C:\\Users\\u\\AppData\\Roaming"), &home());
        assert_eq!(paths.len(), 3);
        assert_eq!(
            paths[2],
            PathBuf::from("C:\\Users\\u\\AppData\\Roaming")
                .join("CherryStudio")
                .join("Data")
                .join("cherrystudio.sqlite")
        );
    }

    #[test]
    fn default_paths_cover_linux_and_macos() {
        let paths = cherrystudio_db_paths(None, &home());
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0],
            home()
                .join(".config")
                .join("CherryStudio")
                .join("Data")
                .join("cherrystudio.sqlite")
        );
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-cherrystudio-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn build_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT PRIMARY KEY,
                input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                reasoning_tokens INTEGER,
                no_cache_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                model_id TEXT,
                model_name TEXT,
                provider_id TEXT,
                provider_name TEXT,
                created_at INTEGER,
                message_kind TEXT,
                message_id TEXT,
                record_kind TEXT
            );
            -- an invocation row with cache splits + reasoning
            INSERT INTO ai_usage_record VALUES (
                'inv1', 100, 40, 140, 10, 5, 20, 8,
                'claude-sonnet-4-5', 'Claude Sonnet 4.5', 'anthropic', 'Anthropic',
                1754042400000, 'chat', 'msg-inv1', 'invocation'
            );
            -- a legacy-aggregate row (pre-migration rollup) that must survive
            INSERT INTO ai_usage_record VALUES (
                'agg1', 1000, 500, 1500, 0, 0, 0, 0,
                'gpt-4o', 'GPT-4o', 'openai', 'OpenAI',
                1750000000000, 'chat', 'msg-agg1', 'legacy-aggregate'
            );
            -- a zero-token row that is dropped
            INSERT INTO ai_usage_record VALUES (
                'zero1', 0, 0, 0, 0, 0, 0, 0,
                'gpt-4o', 'GPT-4o', 'openai', 'OpenAI',
                1750000000000, 'chat', 'msg-zero', 'invocation'
            );",
        )
        .unwrap();
    }

    #[test]
    fn reads_invocation_and_legacy_aggregate_with_cache_splits() {
        let dir = tmp_dir("both-kinds");
        let db = dir.join("cherrystudio.sqlite");
        build_db(&db);
        let result = read_cherrystudio(std::slice::from_ref(&db));
        assert!(result.warnings.is_empty(), "no warnings expected");

        let mut events = result.events;
        events.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        assert_eq!(events.len(), 2, "invocation + legacy-aggregate both emit");

        let agg = &events[0];
        assert_eq!(agg.session_id, "agg1");
        assert_eq!(agg.model, "openai/gpt-4o");
        assert_eq!(agg.tokens.input, 1000);
        assert_eq!(agg.tokens.output, 500);

        let inv = &events[1];
        assert_eq!(inv.session_id, "inv1");
        assert_eq!(inv.message_id, "msg-inv1");
        assert_eq!(inv.model, "anthropic/claude-sonnet-4-5");
        assert_eq!(inv.tokens.input, 100);
        assert_eq!(inv.tokens.output, 40);
        assert_eq!(inv.tokens.reasoning, 10);
        assert_eq!(inv.tokens.cache_read, 20);
        assert_eq!(inv.tokens.cache_write, 8);
        // created_at is epoch millis -> ISO
        assert_eq!(inv.timestamp, "2025-08-01T10:00:00.000Z");

        fs::remove_dir_all(&dir).ok();
    }

    // Build a DB with an explicit set of `(id, in, out, reason, cr, cw, kind,
    // created_at)` rows so tests can insert malformed cells. Columns are declared
    // without CHECK constraints and SQLite's dynamic typing lets us store the
    // malformed text / negative values a drifted real DB could hold.
    fn build_db_with(path: &Path, rows: &[&str]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT PRIMARY KEY,
                input_tokens,
                output_tokens,
                reasoning_tokens,
                cache_read_tokens,
                cache_write_tokens,
                model_id TEXT,
                model_name TEXT,
                provider_id TEXT,
                created_at,
                message_kind TEXT,
                message_id TEXT,
                record_kind TEXT
            );",
        )
        .unwrap();
        for row in rows {
            conn.execute_batch(&format!("INSERT INTO ai_usage_record VALUES ({row});"))
                .unwrap();
        }
    }

    #[test]
    fn missing_db_yields_no_events() {
        let dir = tmp_dir("absent");
        let db = dir.join("cherrystudio.sqlite");
        assert!(read_cherrystudio(&[db]).events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    // Overlap-shaped fixture: a legacy-aggregate and two invocation rows sharing
    // the same message_id and created_at (the exact overlap a naive same-scope
    // dedup would collapse). The proven-disjoint rule emits all three, deduped
    // only by row id, so no real usage is lost or double counted.
    #[test]
    fn overlapping_aggregate_and_invocations_all_survive() {
        let dir = tmp_dir("overlap");
        let db = dir.join("cherrystudio.sqlite");
        build_db_with(
            &db,
            &[
                "'agg1', 1000, 500, 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-shared', 'legacy-aggregate'",
                "'inv1', 100, 40, 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-shared', 'invocation'",
                "'inv2', 60, 20, 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-shared', 'invocation'",
            ],
        );
        let result = read_cherrystudio(std::slice::from_ref(&db));
        assert!(result.warnings.is_empty(), "no warnings expected");
        let mut ids: Vec<&str> = result
            .events
            .iter()
            .map(|e| e.session_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            ["agg1", "inv1", "inv2"],
            "aggregate + both invocations all emit despite shared scope"
        );
        fs::remove_dir_all(&dir).ok();
    }

    // A populated but undecodable token cell (non-numeric text) skips the row
    // with the standard diagnostic; NULL/absent cells still map to 0.
    #[test]
    fn malformed_numeric_text_skips_row() {
        let dir = tmp_dir("malformed-text");
        let db = dir.join("cherrystudio.sqlite");
        build_db_with(
            &db,
            &[
                "'bad1', 'broken', 40, 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-bad', 'invocation'",
                "'ok1', 100, 40, 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-ok', 'invocation'",
            ],
        );
        let result = read_cherrystudio(std::slice::from_ref(&db));
        assert_eq!(result.events.len(), 1, "only the valid row survives");
        assert_eq!(result.events[0].session_id, "ok1");
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![format!("{db_str}:bad1")]);
        assert_eq!(
            result.warnings.len(),
            1,
            "one malformed diagnostic expected"
        );
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed cherrystudio record {db_str}:bad1\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    // A fractional token count is schema-invalid (Cherry Studio enforces
    // typeof = 'integer') and skips the row rather than silently truncating.
    #[test]
    fn fractional_token_count_skips_row() {
        let dir = tmp_dir("fractional");
        let db = dir.join("cherrystudio.sqlite");
        build_db_with(
            &db,
            &[
                "'frac1', 1.9, 40, 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-frac', 'invocation'",
                "'frac2', 100, '1.9', 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-frac2', 'invocation'",
            ],
        );
        let result = read_cherrystudio(std::slice::from_ref(&db));
        assert!(result.events.is_empty(), "both fractional rows are skipped");
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(
            result.skipped,
            vec![format!("{db_str}:frac1"), format!("{db_str}:frac2")]
        );
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed cherrystudio record {db_str}:frac1\n")
        );
        assert_eq!(
            result.warnings[1].message,
            format!("skipping malformed cherrystudio record {db_str}:frac2\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    // A negative token count is invalid (Cherry Studio's schema forbids it) and
    // skips the row rather than silently clamping to 0.
    #[test]
    fn negative_token_count_skips_row() {
        let dir = tmp_dir("negative");
        let db = dir.join("cherrystudio.sqlite");
        build_db_with(
            &db,
            &["'neg1', 100, -5, 0, 0, 0, 'gpt-4o', 'GPT-4o', 'openai', \
                 1750000000000, 'chat', 'msg-neg', 'invocation'"],
        );
        let result = read_cherrystudio(std::slice::from_ref(&db));
        assert!(result.events.is_empty(), "the negative row is skipped");
        let db_str = db.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![format!("{db_str}:neg1")]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed cherrystudio record {db_str}:neg1\n")
        );
        fs::remove_dir_all(&dir).ok();
    }
}

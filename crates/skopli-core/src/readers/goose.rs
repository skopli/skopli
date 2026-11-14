use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, epoch_to_iso, finite_number, reparse_iso, scalar_text,
};
use super::sqlite_store::{open_readonly, query_rows};
use crate::types::{TokenCounts, UsageEvent};

/// The goose reader wired into the harness registry. It reads one cumulative
/// row per session from the goose `sessions.db`.
pub struct GooseReader;

impl Reader for GooseReader {
    fn harness_id(&self) -> &'static str {
        "goose"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let paths = goose_db_paths(ctx.env("GOOSE_PATH_ROOT"), ctx.env("APPDATA"), ctx.home());
        read_goose(&paths)
    }
}

/// Resolve the goose database paths. `GOOSE_PATH_ROOT` is a single path (never
/// comma-split). On Windows goose writes under `%APPDATA%\Block\goose\data`,
/// so that root is tried when `APPDATA` is set and as `AppData/Roaming` under
/// the home.
pub fn goose_db_paths(
    override_val: Option<&str>,
    appdata: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(v) = override_val
        && !v.trim().is_empty()
    {
        return vec![
            PathBuf::from(v.trim())
                .join("data")
                .join("sessions")
                .join("sessions.db"),
        ];
    }
    let mut paths = vec![
        home.join(".local")
            .join("share")
            .join("goose")
            .join("sessions")
            .join("sessions.db"),
        home.join("Library")
            .join("Application Support")
            .join("goose")
            .join("sessions")
            .join("sessions.db"),
        home.join(".local")
            .join("share")
            .join("Block")
            .join("goose")
            .join("sessions")
            .join("sessions.db"),
        windows_sessions_db(&home.join("AppData").join("Roaming")),
    ];
    if let Some(v) = appdata
        && !v.trim().is_empty()
    {
        let path = windows_sessions_db(Path::new(v.trim()));
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn windows_sessions_db(roaming: &Path) -> PathBuf {
    roaming
        .join("Block")
        .join("goose")
        .join("data")
        .join("sessions")
        .join("sessions.db")
}

/// Resolve a `created_at` cell to an ISO string: a string is reparsed as a
/// date-time, anything else is read as an epoch value.
fn created_at_iso(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return reparse_iso(s);
    }
    epoch_to_iso(value)
}

/// Newer goose builds add `cache_read_tokens` and `cache_write_tokens` (plus
/// their `accumulated_` forms) to `sessions` and count them inside
/// `input_tokens`; older databases have neither column.
fn accumulated_or(row: &Value, column: &str) -> f64 {
    row.get(format!("accumulated_{column}"))
        .and_then(finite_number)
        .or_else(|| row.get(column).and_then(finite_number))
        .unwrap_or(0.0)
        .max(0.0)
}

/// Read all goose usage events from the session databases.
pub fn read_goose(db_paths: &[PathBuf]) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen_sessions: HashSet<String> = HashSet::new();

    for db_path in db_paths {
        if !db_path.exists() {
            continue;
        }
        let db_str = db_path.to_string_lossy().into_owned();
        let conn = match open_readonly(&db_str) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read goose database {db_str}: {e}\n"),
                });
                skipped.push(db_str);
                continue;
            }
        };
        let rows = match query_rows(
            &conn,
            "SELECT * FROM sessions WHERE model_config_json IS NOT NULL AND TRIM(model_config_json) != ''",
        ) {
            Ok(r) => r,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read goose database {db_str}: {e}\n"),
                });
                skipped.push(db_str);
                continue;
            }
        };
        for row in &rows {
            if !row.is_object() {
                continue;
            }
            let session_id = scalar_text(row.get("id"));
            if session_id.is_empty() || seen_sessions.contains(&session_id) {
                continue;
            }
            let label = format!("{db_str}:{session_id}");
            let config_str = match row.get("model_config_json") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            let config: Value = match serde_json::from_str(&config_str) {
                Ok(v) => v,
                Err(_) => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed goose session {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            let model = if config.is_object() {
                config.get("model_name").and_then(as_string)
            } else {
                None
            };
            let model = match model {
                Some(m) if !m.trim().is_empty() => m,
                _ => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed goose session {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            let input = row
                .get("accumulated_input_tokens")
                .and_then(finite_number)
                .or_else(|| row.get("input_tokens").and_then(finite_number));
            let output = row
                .get("accumulated_output_tokens")
                .and_then(finite_number)
                .or_else(|| row.get("output_tokens").and_then(finite_number));
            let cache_read = accumulated_or(row, "cache_read_tokens");
            let cache_write = accumulated_or(row, "cache_write_tokens");
            let safe_input = (input.unwrap_or(0.0) - cache_read - cache_write).max(0.0);
            let safe_output = output.unwrap_or(0.0).max(0.0);
            let total = row
                .get("accumulated_total_tokens")
                .and_then(finite_number)
                .or_else(|| row.get("total_tokens").and_then(finite_number))
                .unwrap_or(safe_input + safe_output)
                .max(0.0);
            if safe_input == 0.0
                && safe_output == 0.0
                && cache_read == 0.0
                && cache_write == 0.0
                && total == 0.0
            {
                continue;
            }
            let timestamp = match row.get("created_at").and_then(created_at_iso) {
                Some(t) => t,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed goose session {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            seen_sessions.insert(session_id.clone());
            let provider = row.get("provider_name").and_then(as_string);
            let model_trimmed = model.trim().to_owned();
            let full_model = match provider {
                Some(p) if !p.is_empty() => format!("{p}/{model_trimmed}"),
                _ => model_trimmed,
            };
            events.push(UsageEvent {
                harness: "goose".to_owned(),
                timestamp,
                session_id: session_id.clone(),
                message_id: session_id,
                turn: false,
                subagent: false,
                model: full_model,
                tokens: TokenCounts {
                    input: safe_input as u64,
                    output: safe_output as u64,
                    cache_read: cache_read as u64,
                    cache_write: cache_write as u64,
                    cache_write1h: None,
                    reasoning: (total - safe_input - safe_output - cache_read - cache_write)
                        .max(0.0) as u64,
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
    use crate::readers::shared::scratch_dir;
    use crate::readers::shared::test_home;
    use rusqlite::Connection;

    #[test]
    fn cached_only_session_with_null_totals_keeps_its_cache_read() {
        let dir = scratch_dir("goose-cached-only");
        let db = dir.join("sessions.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT, model_config_json TEXT, provider_name TEXT, created_at TEXT, total_tokens INTEGER, input_tokens INTEGER, output_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER);
             INSERT INTO sessions VALUES ('20261101_2', '{\"model_name\":\"qwen\"}', 'anthropic', '2026-11-01 22:06:30', NULL, 9163, 0, 9163, 0);",
        )
        .unwrap();
        let result = read_goose(&[db]);
        assert_eq!(result.events.len(), 1);
        let tokens = &result.events[0].tokens;
        assert_eq!(
            (tokens.input, tokens.output, tokens.cache_read),
            (0, 0, 9163)
        );
    }

    #[test]
    fn session_cache_columns_split_the_cache_inclusive_input() {
        let dir = scratch_dir("goose-cache");
        let db = dir.join("sessions.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT, model_config_json TEXT, provider_name TEXT, created_at TEXT, total_tokens INTEGER, input_tokens INTEGER, output_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER, accumulated_total_tokens INTEGER, accumulated_input_tokens INTEGER, accumulated_output_tokens INTEGER, accumulated_cache_read_tokens INTEGER, accumulated_cache_write_tokens INTEGER);
             INSERT INTO sessions VALUES ('20261101_1', '{\"model_name\":\"qwen\"}', 'anthropic', '2026-11-01 22:05:30', 13101, 13090, 11, 9163, 3927, 13101, 13090, 11, 9163, 3927);",
        )
        .unwrap();
        let result = read_goose(&[db]);
        assert_eq!(result.events.len(), 1);
        let tokens = &result.events[0].tokens;
        assert_eq!(
            (
                tokens.input,
                tokens.output,
                tokens.cache_read,
                tokens.cache_write
            ),
            (0, 11, 9163, 3927)
        );
    }

    #[test]
    fn default_paths_include_the_windows_roaming_store() {
        let home = test_home();
        let paths = goose_db_paths(None, None, &home);
        assert_eq!(paths.len(), 4);
        assert_eq!(
            paths[3],
            home.join("AppData")
                .join("Roaming")
                .join("Block")
                .join("goose")
                .join("data")
                .join("sessions")
                .join("sessions.db")
        );
    }

    #[test]
    fn appdata_adds_its_own_roaming_store() {
        let home = test_home();
        let paths = goose_db_paths(None, Some("D:/Roaming"), &home);
        assert_eq!(paths.len(), 5);
        assert_eq!(
            paths[4],
            Path::new("D:/Roaming")
                .join("Block")
                .join("goose")
                .join("data")
                .join("sessions")
                .join("sessions.db")
        );
    }

    #[test]
    fn appdata_under_the_home_is_not_opened_twice() {
        let home = test_home();
        let roaming = home.join("AppData").join("Roaming");
        let paths = goose_db_paths(None, roaming.to_str(), &home);
        assert_eq!(paths.len(), 4);
    }

    #[test]
    fn override_wins_over_every_default() {
        let paths = goose_db_paths(Some("/opt/goose"), Some("D:/Roaming"), &test_home());
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with(Path::new("data").join("sessions").join("sessions.db")));
    }
}

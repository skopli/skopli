use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, epoch_to_iso, finite_number, reparse_iso,
};
use super::sqlite_store::{open_readonly, query_rows};
use crate::types::{TokenCounts, UsageEvent};

/// The goose reader wired into the harness registry. It reads one cumulative
/// row per session from the goose `sessions.db`. Faithful port of `readGoose`.
pub struct GooseReader;

impl Reader for GooseReader {
    fn harness_id(&self) -> &'static str {
        "goose"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let paths = goose_db_paths(ctx.env("GOOSE_PATH_ROOT"), ctx.home());
        read_goose(&paths)
    }
}

/// Resolve the goose database paths. Faithful port of `gooseDbPaths`:
/// `GOOSE_PATH_ROOT` is a single path (never comma-split).
pub fn goose_db_paths(override_val: Option<&str>, home: &Path) -> Vec<PathBuf> {
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
    vec![
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
    ]
}

/// Resolve a `created_at` cell to an ISO string. Faithful port of `createdAtIso`:
/// a string goes through `Date.parse`; anything else through `epochToIso`.
fn created_at_iso(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return reparse_iso(s);
    }
    epoch_to_iso(value)
}

/// The `String(fields["id"] ?? "")` fallback for a non-string id.
fn id_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Read all goose usage events. Faithful port of `readGoose`.
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
            "SELECT id, model_config_json, provider_name, created_at, total_tokens, \
             input_tokens, output_tokens, accumulated_total_tokens, \
             accumulated_input_tokens, accumulated_output_tokens FROM sessions \
             WHERE model_config_json IS NOT NULL AND TRIM(model_config_json) != ''",
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
            let session_id = row
                .get("id")
                .and_then(as_string)
                .unwrap_or_else(|| id_string(row.get("id")));
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
            let safe_input = input.unwrap_or(0.0).max(0.0);
            let safe_output = output.unwrap_or(0.0).max(0.0);
            let total = row
                .get("accumulated_total_tokens")
                .and_then(finite_number)
                .or_else(|| row.get("total_tokens").and_then(finite_number))
                .unwrap_or(safe_input + safe_output)
                .max(0.0);
            if safe_input == 0.0 && safe_output == 0.0 && total == 0.0 {
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
                    cache_read: 0,
                    cache_write: 0,
                    cache_write1h: None,
                    reasoning: (total - safe_input - safe_output).max(0.0) as u64,
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

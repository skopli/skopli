use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, finite_number, is_record, parse_timestamp_to_iso,
};
use super::sqlite_store::{blob_column, open_readonly, table_columns};
use crate::types::{TokenCounts, UsageEvent};

// only threads whose provider is Zed's hosted service are counted
const ZED_HOSTED_PROVIDER: &str = "zed.dev";

/// The zed reader wired into the harness registry. It reads one cumulative row
/// per hosted thread from the zed `threads.db`, decoding json/zstd blobs.
/// Faithful port of `readZed`. (No golden case ships for zed - it is
/// SQLite-only - so this lane is not conformance-covered.)
pub struct ZedReader;

impl Reader for ZedReader {
    fn harness_id(&self) -> &'static str {
        "zed"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let paths = zed_db_paths(ctx.env("ZED_DATA_DIR"), ctx.env("LOCALAPPDATA"), ctx.home());
        read_zed(&paths)
    }
}

/// Resolve the zed database paths. Faithful port of `zedDbPaths`.
pub fn zed_db_paths(
    data_dir: Option<&str>,
    localappdata: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(v) = data_dir
        && !v.trim().is_empty()
    {
        return vec![PathBuf::from(v.trim()).join("threads").join("threads.db")];
    }
    let mut paths = vec![
        home.join(".local")
            .join("share")
            .join("zed")
            .join("threads")
            .join("threads.db"),
        home.join("Library")
            .join("Application Support")
            .join("Zed")
            .join("threads")
            .join("threads.db"),
    ];
    if let Some(local) = localappdata
        && !local.is_empty()
    {
        paths.push(
            PathBuf::from(local)
                .join("Zed")
                .join("threads")
                .join("threads.db"),
        );
    }
    paths
}

/// A finite/positive numeric field, else 0, coercing numeric strings. Faithful
/// port of `numericField`.
fn numeric_field(value: Option<&Value>) -> u64 {
    if let Some(v) = value {
        if let Some(n) = finite_number(v) {
            return n.max(0.0) as u64;
        }
        if let Some(s) = v.as_str()
            && let Ok(parsed) = s.parse::<f64>()
            && parsed.is_finite()
        {
            return parsed.max(0.0) as u64;
        }
    }
    0
}

/// Accumulate one usage object into `target`. Faithful port of `addUsage`.
fn add_usage(target: &mut TokenCounts, usage: &Value) {
    target.input += numeric_field(usage.get("input_tokens"));
    target.output += numeric_field(usage.get("output_tokens"));
    target.cache_read += numeric_field(usage.get("cache_read_input_tokens"));
    target.cache_write += numeric_field(usage.get("cache_creation_input_tokens"));
}

/// Sum request-usage entries (array, usage-map, or single usage object).
/// Faithful port of `sumRequestUsage`.
fn sum_request_usage(value: Option<&Value>) -> (TokenCounts, u64) {
    let mut tokens = TokenCounts {
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cache_write1h: None,
        reasoning: 0,
    };
    let mut count = 0u64;
    match value {
        Some(Value::Array(arr)) => {
            for entry in arr {
                if is_record(entry) {
                    add_usage(&mut tokens, entry);
                    count += 1;
                }
            }
        }
        Some(v) if is_record(v) => {
            let looks_like_usage = v.get("input_tokens").is_some()
                || v.get("output_tokens").is_some()
                || v.get("cache_read_input_tokens").is_some()
                || v.get("cache_creation_input_tokens").is_some();
            if looks_like_usage {
                add_usage(&mut tokens, v);
                count += 1;
            } else if let Value::Object(map) = v {
                for entry in map.values() {
                    if is_record(entry) {
                        add_usage(&mut tokens, entry);
                        count += 1;
                    }
                }
            }
        }
        _ => {}
    }
    (tokens, count)
}

fn tokens_are_zero(tokens: &TokenCounts) -> bool {
    tokens.input == 0 && tokens.output == 0 && tokens.cache_read == 0 && tokens.cache_write == 0
}

/// Decode a thread blob. Faithful port of `decodeBlob`: only `json`/`zstd`
/// (or NULL data_type) are accepted; anything else yields `None`. A decode /
/// JSON-parse failure surfaces as `Err(())` (the TS `try/catch` skip path).
fn decode_blob(data_type: Option<&str>, data: Option<Vec<u8>>) -> Result<Option<Value>, ()> {
    if let Some(dt) = data_type
        && dt != "json"
        && dt != "zstd"
    {
        return Ok(None);
    }
    let bytes = match data {
        Some(b) => b,
        None => return Ok(None),
    };
    let text = if data_type == Some("zstd") {
        let mut decoder =
            ruzstd::decoding::StreamingDecoder::new(bytes.as_slice()).map_err(|_| ())?;
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).map_err(|_| ())?;
        String::from_utf8(out).map_err(|_| ())?
    } else {
        String::from_utf8(bytes).map_err(|_| ())?
    };
    let value: Value = serde_json::from_str(&text).map_err(|_| ())?;
    Ok(Some(value))
}

/// The `String(row["id"] ?? "")` fallback for a non-string thread id.
fn id_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Read all zed usage events. Faithful port of `readZed`.
pub fn read_zed(db_paths: &[PathBuf]) -> ReaderResult {
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
                    message: format!("unable to read zed database {db_str}: {e}\n"),
                });
                skipped.push(db_str);
                continue;
            }
        };
        let names = table_columns(&conn, "threads");
        let created = if names.contains("created_at") {
            "created_at"
        } else {
            "NULL AS created_at"
        };
        let query = format!("SELECT id, updated_at, {created}, data_type, data FROM threads");
        let mut stmt = match conn.prepare(&query) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read zed database {db_str}: {e}\n"),
                });
                skipped.push(db_str);
                continue;
            }
        };
        let column_names: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let mut rows = match stmt.query([]) {
            Ok(r) => r,
            Err(e) => {
                warnings.push(ReaderWarning {
                    message: format!("unable to read zed database {db_str}: {e}\n"),
                });
                skipped.push(db_str);
                continue;
            }
        };

        while let Ok(Some(row)) = rows.next() {
            // Build a JSON row for the scalar columns, keep `data` as raw bytes.
            let mut obj = serde_json::Map::new();
            let mut data_bytes: Option<Vec<u8>> = None;
            for (i, name) in column_names.iter().enumerate() {
                let cell = match row.get_ref(i) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if name == "data" {
                    data_bytes = blob_column(cell);
                    obj.insert(name.clone(), Value::Null);
                } else {
                    obj.insert(name.clone(), super::sqlite_store::cell_to_value(cell));
                }
            }
            let row_val = Value::Object(obj);

            let thread_id = row_val
                .get("id")
                .and_then(as_string)
                .unwrap_or_else(|| id_string(row_val.get("id")));
            if thread_id.is_empty() || seen.contains(&thread_id) {
                continue;
            }
            let label = format!("{db_str}:{thread_id}");
            let data_type = row_val.get("data_type").and_then(as_string);
            let blob = match decode_blob(data_type.as_deref(), data_bytes) {
                Ok(b) => b,
                Err(()) => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed zed thread {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            let blob = match blob {
                Some(b) if is_record(&b) => b,
                _ => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed zed thread {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            if blob.get("imported") == Some(&Value::Bool(true)) {
                continue;
            }
            let model = blob.get("model").filter(|m| is_record(m));
            let provider = model.and_then(|m| m.get("provider").and_then(as_string));
            let model_name = model.and_then(|m| m.get("model").and_then(as_string));
            let provider_ok = provider
                .as_deref()
                .map(|p| p.to_lowercase() == ZED_HOSTED_PROVIDER)
                .unwrap_or(false);
            let model_name = match model_name {
                Some(m) if !m.trim().is_empty() => m,
                _ => continue,
            };
            if !provider_ok {
                continue;
            }

            let (request_tokens, request_count) =
                sum_request_usage(blob.get("request_token_usage"));
            let mut tokens = request_tokens;
            if tokens_are_zero(&tokens) {
                tokens = sum_request_usage(blob.get("cumulative_token_usage")).0;
            }
            if tokens_are_zero(&tokens) {
                continue;
            }
            let count = request_count;
            let timestamp = parse_ts(row_val.get("updated_at"))
                .or_else(|| parse_ts(blob.get("updated_at")))
                .or_else(|| parse_ts(row_val.get("created_at")));
            let timestamp = match timestamp {
                Some(t) => t,
                None => {
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed zed thread {label}\n"),
                    });
                    skipped.push(label);
                    continue;
                }
            };
            seen.insert(thread_id.clone());
            events.push(UsageEvent {
                harness: "zed".to_owned(),
                timestamp,
                session_id: thread_id.clone(),
                message_id: thread_id,
                turn: false,
                subagent: false,
                model: format!("{ZED_HOSTED_PROVIDER}/{}", model_name.trim()),
                tokens,
                calls: if count > 0 { Some(count) } else { None },
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

/// `parseTimestampToIso` for an optional value (None -> None).
fn parse_ts(value: Option<&Value>) -> Option<String> {
    value.and_then(parse_timestamp_to_iso)
}

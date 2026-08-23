//! SQLite helpers for the SQLite-backed readers. Mirrors `src/sqlite-message-store.ts`
//! (the shared message-store loop) plus the small open/query surface the TS
//! readers get from `node:sqlite`.
//!
//! Databases are opened `READ_ONLY | NO_MUTEX` (spec toolchain pin) so the
//! committed golden `.db` binaries are never mutated and WAL sidecars still
//! read. Column values are decoded into `serde_json::Value` so the readers can
//! reuse the same `as_string`/`finite_number`/`is_record` helpers the JSONL
//! lanes use.

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use super::shared::{ReaderResult, ReaderWarning, as_string};

/// Open a SQLite database `READ_ONLY | NO_MUTEX`, the faithful analogue of the
/// TS readers' `new DatabaseSync(path, { readOnly: true })`. Returns an error
/// (never panics) when the file cannot be opened.
pub fn open_readonly(path: &str) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// Convert a single SQLite column value to a `serde_json::Value`, matching how
/// `node:sqlite` surfaces cells to JS: NULL -> null, INTEGER/REAL -> number,
/// TEXT -> string, BLOB -> an array of byte numbers is NOT produced (readers
/// that need raw bytes use [`blob_column`]); here a BLOB maps to null so the
/// `as_string`/`finite_number` helpers treat it as absent.
pub fn cell_to_value(cell: ValueRef<'_>) -> Value {
    match cell {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(t) => match std::str::from_utf8(t) {
            Ok(s) => Value::String(s.to_owned()),
            Err(_) => Value::Null,
        },
        ValueRef::Blob(_) => Value::Null,
    }
}

/// Run `query` and return each row as a JSON object keyed by column name. Column
/// order and names come from the prepared statement. Errors propagate to the
/// caller (the readers translate a query error into their own diagnostic).
pub fn query_rows(conn: &Connection, query: &str) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(query)?;
    let column_names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut obj = serde_json::Map::with_capacity(column_names.len());
        for (i, name) in column_names.iter().enumerate() {
            obj.insert(name.clone(), cell_to_value(row.get_ref(i)?));
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

/// A row from a message-store query, mirroring `SqliteMessageRow` in
/// src/sqlite-message-store.ts: the JSON object of selected columns.
pub type SqliteMessageRow = Value;

/// The parse outcome for a single message-store row (analogue of the TS
/// `{ value, malformed }`): `value == None && !malformed` means "skip quietly".
pub struct ParsedValue<T> {
    pub value: Option<T>,
    pub malformed: bool,
}

/// Options for [`read_sqlite_message_store`], mirroring
/// `SqliteMessageStoreOptions<T>`.
pub struct SqliteMessageStoreOptions<'a, T> {
    pub db_path: &'a str,
    pub query: &'a str,
    /// Parse a row + its already-JSON-parsed `data` blob into an optional value.
    pub parse: &'a mut dyn FnMut(&SqliteMessageRow, &Value) -> ParsedValue<T>,
    pub malformed_label: &'a str,
}

/// Faithful port of `readSqliteMessageStore` in src/sqlite-message-store.ts:
/// open the db read-only, run `query`, and for each row decode the string
/// `data` column as JSON, invoking `parse`. A missing/non-string `data`, a JSON
/// parse failure, or a `malformed` parse result each warn + record
/// `<dbPath>:<id>` in `skipped`. Returns the collected values plus the result
/// (events stay empty here; the caller assembles events).
///
/// The `Err` case corresponds to the TS `readSqliteMessageStore` throwing
/// (an unopenable/failed query): the caller decides how to surface it.
pub fn read_sqlite_message_store<T>(
    opts: SqliteMessageStoreOptions<'_, T>,
) -> rusqlite::Result<(Vec<T>, ReaderResult)> {
    let conn = open_readonly(opts.db_path)?;
    let rows = query_rows(&conn, opts.query)?;
    let mut values: Vec<T> = Vec::new();
    let mut result = ReaderResult::default();
    for row in &rows {
        let id = row
            .get("id")
            .and_then(as_string)
            .unwrap_or_else(|| "unknown".to_owned());
        let mark = |result: &mut ReaderResult| {
            let loc = format!("{}:{}", opts.db_path, id);
            result.skipped.push(loc);
            result.warnings.push(ReaderWarning {
                message: format!(
                    "skipping malformed {} {}:{}\n",
                    opts.malformed_label, opts.db_path, id
                ),
            });
        };
        let data = match row.get("data").and_then(|v| v.as_str().map(str::to_owned)) {
            Some(d) => d,
            None => {
                mark(&mut result);
                continue;
            }
        };
        let raw: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => {
                mark(&mut result);
                continue;
            }
        };
        let parsed = (opts.parse)(row, &raw);
        if parsed.malformed {
            mark(&mut result);
        } else if let Some(value) = parsed.value {
            values.push(value);
        }
    }
    Ok((values, result))
}

/// Read an integer `PRAGMA` (for example `application_id` or `user_version`).
/// Returns `None` when the pragma cannot be read. Mirrors the TS readers'
/// `db.prepare("PRAGMA ...").get()` integer probes.
pub fn pragma_int(conn: &Connection, pragma: &str) -> Option<i64> {
    let sql = format!("PRAGMA {pragma}");
    conn.query_row(&sql, [], |row| row.get::<_, i64>(0)).ok()
}

/// True when the given table exists in the database (`sqlite_master` probe).
/// Faithful analogue of the TS readers' `SELECT 1 FROM sqlite_master ...`
/// existence probes. A query error is treated as "not present".
pub fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |_| Ok(()),
    )
    .is_ok()
}

/// The set of column names for `table` via `PRAGMA table_info`. Empty on error.
/// Mirrors the TS `PRAGMA table_info(...)` column scans.
pub fn table_columns(conn: &Connection, table: &str) -> std::collections::HashSet<String> {
    let mut cols = std::collections::HashSet::new();
    let sql = format!("PRAGMA table_info({table})");
    if let Ok(mut stmt) = conn.prepare(&sql)
        && let Ok(mut rows) = stmt.query([])
    {
        while let Ok(Some(row)) = rows.next() {
            if let Ok(name) = row.get::<_, String>(1) {
                cols.insert(name);
            }
        }
    }
    cols
}

/// Read a BLOB/TEXT column as raw bytes (used by the zed reader, which decodes
/// json/zstd blobs). Returns `None` when the cell is NULL.
pub fn blob_column(cell: ValueRef<'_>) -> Option<Vec<u8>> {
    match cell {
        ValueRef::Blob(b) => Some(b.to_vec()),
        ValueRef::Text(t) => Some(t.to_vec()),
        _ => None,
    }
}

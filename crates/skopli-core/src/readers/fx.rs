use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{ReaderResult, ReaderWarning, as_string, is_record, parse_timestamp_to_iso};
use crate::types::{TokenCounts, UsageEvent};

/// The fx.sh reader wired into the harness registry. fx (vercel-labs/fx) is a
/// native coding agent that persists per-session usage at
/// `~/.fx/sessions/<session_id>/usage-v2.json`. The profile root constant is
/// `.fx`, joined to the OS home; there is no environment override that
/// redirects it, so the root resolves from home only.
///
/// Discovery enumerates only the immediate session directories under
/// `sessions/`, sorted, and inspects the single fixed `usage-v2.json` directly
/// below each one. Files at any deeper nesting are ignored: fx writes exactly one
/// sidecar per session directory, so a deterministic one-based ordinal over the
/// sorted session directories is the diagnostic index.
///
/// Validation is scoped to the fields the reader consumes, not the full E11
/// producer schema. Every other snapshot and model field (`first_sequence`,
/// `billable_web_search_calls`, the snapshot totals, the duration/cursor/boolean
/// fields, `pending`, `publication_backlog`, and so on) is intentionally not
/// gated: strict full-schema validation would reject benign producer additions
/// without improving usage correctness. A violation of a validated field skips
/// that sidecar and yields one structured diagnostic against a
/// `{path}:usage-v2:{ordinal}` identity. The validated fields are:
///
/// - The envelope is an object with exactly the three keys `schema_version`,
///   `session_id`, and `snapshot`; `schema_version` is the integer 1;
///   `session_id` is a non-empty string equal to the owning session directory
///   name and a valid fx session id of the form `{ms}-{ns}-{16hex}` (decimal
///   millis, decimal nanos, and exactly sixteen lowercase hex characters); and
///   `snapshot` is an object.
/// - The snapshot carries its own integer `schema_version` equal to 1, a
///   `billing` string that is one of `complete` / `pending` / `incomplete` /
///   `legacy`, and a `models` array. An empty `models` array is valid and records
///   no usage without a diagnostic.
/// - Every entry of `models[]` is an object with a non-blank `model` string and
///   required u64 token counters `input_tokens`, `output_tokens`,
///   `cache_read_tokens`, and `cache_write_tokens`, each a JSON integer in
///   `0..=u64::MAX`. A required counter that is absent, null, negative,
///   fractional, non-numeric, or above `u64::MAX` makes the whole sidecar
///   malformed rather than coercing to zero. `reasoning_tokens` and
///   `request_count` are optional: absent or null is honored (`legacy` snapshots
///   write both as null, mapping to a zero reasoning count and an absent call
///   count), but a populated value that is not a u64 integer is malformed.
/// - The provider-qualified `model` names within one sidecar must be unique;
///   duplicate names make the whole sidecar malformed, so the
///   `{session_id}:{model}` identity below can never collide.
///
/// The token and cost figures are authoritative provider counts applied from AI
/// Gateway billing records, not estimates. One event is emitted per entry of the
/// snapshot's `models[]` array; each entry carries a provider-qualified model id
/// (`provider/model`) which is stored as-is, matching the devin precedent, plus
/// its own token counts, request count, and cost. The `message_id` is
/// `{session_id}:{model}`, keyed on the provider-qualified model name rather than
/// the array position, which is not evidenced as stable. E11 does not evidence
/// that model names are unique within a snapshot, so the reader enforces it:
/// duplicate model names make the sidecar malformed, keeping the identity
/// collision-free rather than assuming the producer never repeats a row. The
/// event timestamp derives from the session id's leading millisecond prefix.
///
/// Each sidecar is a cumulative whole-snapshot restatement of a session's usage,
/// not an append: a later read of the same session supersedes the earlier one.
/// Correctness across repeated reads therefore requires the consumer to replace
/// prior events by `message_id`; the rollup layer sums events blindly, so
/// re-reading a rewritten snapshot without message-id-keyed replacement would
/// double-count. Keying `message_id` on the stable per-model identity is what
/// makes that replacement well defined.
///
/// The `billing` enum describes reconciliation completeness of the saved window,
/// not the validity of the aggregated per-model figures: the counts are already
/// the authoritative applied totals in every state. All four states are
/// therefore read; usage that exists is usage. A snapshot may also carry an
/// `incidents[]` array recording completeness incidents; when non-empty it draws
/// one informational warning naming the session, but no usage is skipped.
pub struct FxReader;

impl Reader for FxReader {
    fn harness_id(&self) -> &'static str {
        HARNESS
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        read_fx(&fx_sessions_dir(ctx.home()))
    }
}

const HARNESS: &str = "fx";
const SIDECAR_FILE: &str = "usage-v2.json";

/// Resolve the fx sessions directory, `<home>/.fx/sessions`. The profile root
/// (`.fx`) is a fixed home-relative constant on every OS with no environment
/// override, so the sessions directory resolves from home only.
pub fn fx_sessions_dir(home: &Path) -> PathBuf {
    home.join(".fx").join("sessions")
}

/// The four billing states fx records; any other string is malformed.
const BILLING_STATES: [&str; 4] = ["complete", "pending", "incomplete", "legacy"];

/// A required u64 token counter. Returns the value only when it is a JSON integer
/// in `0..=u64::MAX`; a value that is absent, null, negative, fractional,
/// non-numeric, or above `u64::MAX` is `None`, which the caller treats as a
/// malformed sidecar rather than coercing to zero.
fn required_u64(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

/// An optional u64 counter with three outcomes: `Ok(Some)` for a valid u64
/// integer, `Ok(None)` for an absent or null value (fx's honored optionality,
/// as on `legacy` snapshots), and `Err(())` for a populated value that is not a
/// u64 integer (malformed).
fn optional_u64(value: Option<&Value>) -> Result<Option<u64>, ()> {
    match value {
        None => Ok(None),
        Some(v) if v.is_null() => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or(()),
    }
}

/// An optional finite cost. `null`, absent, or non-finite -> `None`. Cost is not
/// a validated required field; a non-finite value simply carries no cost.
fn optional_cost(value: Option<&Value>) -> Option<f64> {
    value?.as_f64().filter(|n| n.is_finite())
}

/// True when `id` is a valid fx session id: `{ms}-{ns}-{16hex}`, that is a
/// decimal millisecond field, a decimal nanosecond field, and exactly sixteen
/// lowercase hexadecimal characters, joined by single hyphens with no extra
/// components. Both numeric fields must be non-empty runs of ASCII digits.
fn is_valid_session_id(id: &str) -> bool {
    let mut parts = id.split('-');
    let (Some(ms), Some(ns), Some(hex), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let is_decimal = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let is_hex16 = hex.len() == 16
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    is_decimal(ms) && is_decimal(ns) && is_hex16
}

/// Derive the session timestamp from a validated session id's leading
/// millisecond prefix. The caller has already confirmed the id shape, so the
/// substring before the first `-` is the epoch-millisecond session start.
fn timestamp_from_session_id(session_id: &str) -> Option<String> {
    let prefix = session_id.split('-').next()?;
    let ms: f64 = prefix.parse().ok()?;
    parse_timestamp_to_iso(&Value::from(ms))
}

/// Validate one `models[]` entry and build its event. Returns `Err(())` when the
/// entry is not an object, the model id is absent or blank, a required u64 token
/// counter is invalid, or an optional counter is populated with a non-u64 value.
/// The provider-qualified model id is stored as-is, and also keys the
/// `message_id` (`{session_id}:{model}`) so the identity is stable across
/// snapshot rewrites rather than depending on array position, which is not
/// evidenced as stable. Uniqueness of model names within one sidecar is enforced
/// by the caller ([`validate_sidecar`]) rather than assumed, so the identity
/// never collides.
fn model_event(entry: &Value, session_id: &str, timestamp: &str) -> Result<UsageEvent, ()> {
    if !is_record(entry) {
        return Err(());
    }
    let model = entry
        .get("model")
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .ok_or(())?;
    let input = required_u64(entry.get("input_tokens")).ok_or(())?;
    let output = required_u64(entry.get("output_tokens")).ok_or(())?;
    let cache_read = required_u64(entry.get("cache_read_tokens")).ok_or(())?;
    let cache_write = required_u64(entry.get("cache_write_tokens")).ok_or(())?;
    let reasoning = optional_u64(entry.get("reasoning_tokens"))?.unwrap_or(0);
    let calls = optional_u64(entry.get("request_count"))?;
    let message_id = format!("{session_id}:{model}");
    Ok(UsageEvent {
        harness: HARNESS.to_owned(),
        timestamp: timestamp.to_owned(),
        session_id: session_id.to_owned(),
        message_id,
        turn: false,
        subagent: false,
        model,
        tokens: TokenCounts {
            input,
            output,
            cache_read,
            cache_write,
            cache_write1h: None,
            reasoning,
        },
        calls,
        cost_usd: optional_cost(entry.get("total_cost")),
        workspace: None,
        title: None,
    })
}

/// The outcome of validating one sidecar: the events it yields and whether its
/// snapshot carried a non-empty `incidents[]` array (an informational signal,
/// not a skip).
struct SidecarRead {
    events: Vec<UsageEvent>,
    has_incidents: bool,
}

/// Validate and read one `usage-v2.json` sidecar into events, or `Err(())` when
/// the sidecar violates any of the fields the reader consumes. Validation is a
/// scoped subset of the E11 producer schema, not the full schema: fields the
/// reader does not consume are intentionally not gated. `dir_name` is the owning
/// session directory name the envelope `session_id` must equal. Enforced:
/// readable file and object JSON; an envelope with exactly the three keys
/// `schema_version` / `session_id` / `snapshot`; outer `schema_version` == 1;
/// `session_id` a non-empty string equal to `dir_name` and a valid fx session
/// id; `snapshot` an object carrying its own `schema_version` == 1, a `billing`
/// string among the four known states, and a `models` array. Every model entry
/// is validated by [`model_event`], and model names must be unique within the
/// sidecar (a duplicate provider-qualified name is malformed, keeping the
/// `{session_id}:{model}` identity collision-free). An empty `models` array is
/// valid and yields no events. A non-empty `incidents[]` is reported through
/// `has_incidents`.
fn validate_sidecar(file: &str, dir_name: &str) -> Result<SidecarRead, ()> {
    let content = std::fs::read_to_string(file).map_err(|_| ())?;
    let envelope: Value = serde_json::from_str(&content).map_err(|_| ())?;
    let envelope = envelope.as_object().ok_or(())?;
    if envelope.len() != 3
        || !envelope.contains_key("schema_version")
        || !envelope.contains_key("session_id")
        || !envelope.contains_key("snapshot")
    {
        return Err(());
    }
    if envelope.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(());
    }
    let session_id = envelope
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or(())?;
    if session_id != dir_name || !is_valid_session_id(session_id) {
        return Err(());
    }
    let snapshot = envelope
        .get("snapshot")
        .and_then(Value::as_object)
        .ok_or(())?;
    if snapshot.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(());
    }
    let billing = snapshot.get("billing").and_then(Value::as_str).ok_or(())?;
    if !BILLING_STATES.contains(&billing) {
        return Err(());
    }
    let models = snapshot.get("models").and_then(Value::as_array).ok_or(())?;
    let timestamp = timestamp_from_session_id(session_id).ok_or(())?;
    let mut events = Vec::with_capacity(models.len());
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in models.iter() {
        let event = model_event(entry, session_id, &timestamp)?;
        if !seen.insert(event.model.clone()) {
            return Err(());
        }
        events.push(event);
    }
    Ok(SidecarRead {
        events,
        has_incidents: has_incidents(snapshot),
    })
}

/// True when the snapshot carries a non-empty `incidents[]` array. `incidents`
/// records completeness incidents that make the snapshot's completeness
/// self-describing (E11); an absent or non-array `incidents` is optional and
/// ignored. When present and non-empty, the caller emits an informational
/// warning naming the session rather than skipping any usage: the per-model
/// figures are still authoritative, the incidents only annotate reconciliation.
fn has_incidents(snapshot: &serde_json::Map<String, Value>) -> bool {
    snapshot
        .get("incidents")
        .and_then(Value::as_array)
        .is_some_and(|incidents| !incidents.is_empty())
}

/// Read one `usage-v2.json` sidecar into events. A schema violation records a
/// single structured diagnostic against `label` and yields no events. A valid
/// sidecar whose snapshot carries a non-empty `incidents[]` array additionally
/// pushes one informational warning naming the session; that warning does not
/// skip any usage and is not added to `skipped`.
fn read_sidecar(
    file: &str,
    dir_name: &str,
    label: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    match validate_sidecar(file, dir_name) {
        Ok(read) => {
            if read.has_incidents {
                warnings.push(ReaderWarning {
                    message: format!(
                        "{HARNESS} session {dir_name} reports completeness incidents\n"
                    ),
                });
            }
            read.events
        }
        Err(()) => {
            skip(label, skipped, warnings);
            Vec::new()
        }
    }
}

/// Push one malformed-sidecar diagnostic for `label`.
fn skip(label: &str, skipped: &mut Vec<String>, warnings: &mut Vec<ReaderWarning>) {
    warnings.push(ReaderWarning {
        message: format!("skipping malformed {HARNESS} record {label}\n"),
    });
    skipped.push(label.to_owned());
}

/// The immediate subdirectory names of `dir`, sorted. Unreadable directories
/// and non-directory entries are ignored. Enumeration stays at depth one so a
/// deeper nested file cannot masquerade as a session or shift ordinals.
fn immediate_session_dirs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// Read all fx usage events from the sessions directory. Only the immediate
/// session directories under `sessions/` are enumerated, sorted, and each one's
/// single fixed `usage-v2.json` is inspected; deeper files are ignored. The
/// `{path}:usage-v2:{ordinal}` diagnostic identity uses the sidecar file path,
/// the `usage-v2` scope, and a one-based ordinal over the sorted session
/// directories. A missing session directory or a directory without its sidecar
/// yields no events and no diagnostics (absence is normal).
pub fn read_fx(sessions_dir: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let mut ordinal = 0usize;
    for name in immediate_session_dirs(sessions_dir) {
        let sidecar = sessions_dir.join(&name).join(SIDECAR_FILE);
        if !sidecar.is_file() {
            continue;
        }
        ordinal += 1;
        let file = sidecar.to_string_lossy().into_owned();
        let label = format!("{file}:usage-v2:{ordinal}");
        events.extend(read_sidecar(
            &file,
            &name,
            &label,
            &mut skipped,
            &mut warnings,
        ));
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
    fn sessions_dir_is_home_dot_fx_sessions() {
        assert_eq!(
            fx_sessions_dir(&home()),
            home().join(".fx").join("sessions")
        );
    }

    #[test]
    fn timestamp_derives_from_session_id_ms_prefix() {
        let id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        assert_eq!(
            timestamp_from_session_id(id).as_deref(),
            Some("2026-02-02T02:40:00.000Z")
        );
    }

    #[test]
    fn valid_session_id_requires_all_three_components() {
        assert!(is_valid_session_id(
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718"
        ));
    }

    #[test]
    fn invalid_session_ids_are_rejected() {
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("nosep"));
        assert!(!is_valid_session_id("abc-1-a1b2c3d4e5f60718"));
        assert!(!is_valid_session_id("1770000000000-x-a1b2c3d4e5f60718"));
        assert!(!is_valid_session_id("1770000000000--a1b2c3d4e5f60718"));
        assert!(!is_valid_session_id("1770000000000-1-not-hex"));
        assert!(!is_valid_session_id(
            "1770000000000-1-a1b2c3d4e5f60718-extra"
        ));
        assert!(!is_valid_session_id("1770000000000-1-a1b2c3d4e5f6071"));
        assert!(!is_valid_session_id("1770000000000-1-A1B2C3D4E5F60718"));
        assert!(!is_valid_session_id("1770000000000-1-a1b2c3d4e5f6071g"));
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-fx-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn write_sidecar(sessions: &Path, session_id: &str, snapshot: Value) {
        let dir = sessions.join(session_id);
        fs::create_dir_all(&dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": session_id,
            "snapshot": snapshot,
        });
        fs::write(dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
    }

    #[test]
    fn empty_sessions_dir_yields_no_events() {
        let dir = tmp_dir("empty");
        let sessions = dir.join("sessions");
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        assert!(result.skipped.is_empty());
        assert!(result.warnings.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn one_event_per_model_entry_with_real_tokens() {
        let dir = tmp_dir("multi-model");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.99,
                "input_tokens": 999,
                "output_tokens": 999,
                "cache_read_tokens": 999,
                "cache_write_tokens": 999,
                "reasoning_tokens": 999,
                "request_count": 99,
                "models": [
                    {
                        "model": "openai/gpt-5.4",
                        "total_cost": 0.03,
                        "input_tokens": 20,
                        "output_tokens": 8,
                        "cache_read_tokens": 4,
                        "cache_write_tokens": 2,
                        "reasoning_tokens": 4,
                        "request_count": 1
                    },
                    {
                        "model": "anthropic/claude-sonnet-4-5",
                        "total_cost": 0.02,
                        "input_tokens": 10,
                        "output_tokens": 4,
                        "cache_read_tokens": 2,
                        "cache_write_tokens": 1,
                        "reasoning_tokens": 0,
                        "request_count": 1
                    }
                ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(events.len(), 2);
        let first = &events[0];
        assert_eq!(first.model, "openai/gpt-5.4");
        assert_eq!(first.tokens.input, 20);
        assert_eq!(first.tokens.output, 8);
        assert_eq!(first.tokens.cache_read, 4);
        assert_eq!(first.tokens.cache_write, 2);
        assert_eq!(first.tokens.reasoning, 4);
        assert_eq!(first.calls, Some(1));
        assert_eq!(first.cost_usd, Some(0.03));
        assert_eq!(first.timestamp, "2026-02-02T02:40:00.000Z");
        assert!(!first.turn);
        assert!(!first.subagent);
        assert_eq!(
            first.session_id,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718"
        );
        assert_eq!(
            first.message_id,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718:openai/gpt-5.4"
        );
        let second = &events[1];
        assert_eq!(
            second.message_id,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718:anthropic/claude-sonnet-4-5"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_snapshot_reads_with_absent_reasoning_and_calls() {
        let dir = tmp_dir("legacy");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000001000-1770000000000000001-b1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "legacy",
                "total_cost": 0.01,
                "input_tokens": 10,
                "output_tokens": 4,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0,
                "reasoning_tokens": null,
                "request_count": null,
                "models": [
                    {
                        "model": "openai/gpt-5.4",
                        "total_cost": 0.01,
                        "input_tokens": 10,
                        "output_tokens": 4,
                        "cache_read_tokens": 0,
                        "cache_write_tokens": 0,
                        "reasoning_tokens": null,
                        "request_count": null
                    }
                ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tokens.reasoning, 0);
        assert_eq!(events[0].calls, None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pending_billing_is_still_read() {
        let dir = tmp_dir("pending");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000002000-1770000000000000002-c1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "pending",
                "total_cost": 0.02,
                "input_tokens": 15,
                "output_tokens": 5,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0,
                "reasoning_tokens": 0,
                "request_count": 1,
                "models": [
                    {
                        "model": "anthropic/claude-sonnet-4-5",
                        "total_cost": 0.02,
                        "input_tokens": 15,
                        "output_tokens": 5,
                        "cache_read_tokens": 0,
                        "cache_write_tokens": 0,
                        "reasoning_tokens": 0,
                        "request_count": 1
                    }
                ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(events.len(), 1, "pending usage that exists is still usage");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_schema_version_is_a_diagnostic() {
        let dir = tmp_dir("bad-version");
        let sessions = dir.join("sessions");
        let session_id = "1770000003000-1770000000000000003-d1b2c3d4e5f60718";
        let session_dir = sessions.join(session_id);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 2,
            "session_id": session_id,
            "snapshot": { "schema_version": 2, "models": [] },
        });
        let file = session_dir.join(SIDECAR_FILE);
        fs::write(&file, envelope.to_string()).unwrap();
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        let file_str = file.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![format!("{file_str}:usage-v2:1")]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed fx record {file_str}:usage-v2:1\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_json_is_a_diagnostic() {
        let dir = tmp_dir("bad-json");
        let sessions = dir.join("sessions");
        let session_dir = sessions.join("1770000004000-1770000000000000004-e1b2c3d4e5f60718");
        fs::create_dir_all(&session_dir).unwrap();
        let file = session_dir.join(SIDECAR_FILE);
        fs::write(&file, b"{not valid json").unwrap();
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        let file_str = file.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![format!("{file_str}:usage-v2:1")]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_models_array_is_silent() {
        let dir = tmp_dir("empty-models");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000005000-1770000000000000005-f1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.0,
                "input_tokens": 0,
                "output_tokens": 0,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0,
                "reasoning_tokens": 0,
                "request_count": 0,
                "models": []
            }),
        );
        let result = read_fx(&sessions);
        assert!(result.events.is_empty());
        assert!(result.warnings.is_empty());
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn incomplete_billing_is_still_read() {
        let dir = tmp_dir("incomplete");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000006000-1770000000000000006-a2b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "incomplete",
                "total_cost": 0.02,
                "models": [ model_aggregate("openai/gpt-5.4") ]
            }),
        );
        let events = read_fx(&sessions).events;
        assert_eq!(
            events.len(),
            1,
            "incomplete usage that exists is still usage"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_empty_incidents_draw_an_informational_warning_without_skipping() {
        let dir = tmp_dir("incidents");
        let sessions = dir.join("sessions");
        let session_id = "1770000007000-1770000000000000007-b2b2c3d4e5f60718";
        write_sidecar(
            &sessions,
            session_id,
            serde_json::json!({
                "schema_version": 1,
                "billing": "incomplete",
                "total_cost": 0.02,
                "incidents": [ { "occurred_at_ms": 1770000007000i64, "completeness": "partial" } ],
                "models": [ model_aggregate("openai/gpt-5.4") ]
            }),
        );
        let result = read_fx(&sessions);
        assert_eq!(result.events.len(), 1, "incidents do not skip usage");
        assert!(result.skipped.is_empty(), "an incident is not a skip");
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result.warnings[0].message,
            format!("fx session {session_id} reports completeness incidents\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_or_absent_incidents_are_silent() {
        let dir = tmp_dir("incidents-empty");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000008000-1770000000000000008-c2b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.02,
                "incidents": [],
                "models": [ model_aggregate("openai/gpt-5.4") ]
            }),
        );
        let result = read_fx(&sessions);
        assert_eq!(result.events.len(), 1);
        assert!(result.warnings.is_empty(), "empty incidents is silent");
        assert!(result.skipped.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_model_names_are_a_diagnostic() {
        let dir = tmp_dir("duplicate-model");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000009000-1770000000000000009-d2b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "total_cost": 0.04,
                "models": [
                    model_aggregate("openai/gpt-5.4"),
                    model_aggregate("openai/gpt-5.4")
                ]
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    /// A well-formed model aggregate whose required u64 counters are all present.
    fn model_aggregate(model: &str) -> Value {
        serde_json::json!({
            "model": model,
            "total_cost": 0.02,
            "input_tokens": 10,
            "output_tokens": 4,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "reasoning_tokens": 0,
            "request_count": 1
        })
    }

    /// A snapshot with one model aggregate mutated by `mutate`, used to prove a
    /// specific malformed field skips the whole sidecar.
    fn snapshot_with_model(mutate: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Value {
        let mut aggregate = model_aggregate("openai/gpt-5.4");
        mutate(aggregate.as_object_mut().unwrap());
        serde_json::json!({
            "schema_version": 1,
            "billing": "complete",
            "total_cost": 0.02,
            "models": [ aggregate ]
        })
    }

    fn assert_only_diagnostic(sessions: &Path) {
        let result = read_fx(sessions);
        assert!(result.events.is_empty(), "expected no events");
        assert_eq!(result.skipped.len(), 1, "expected one skipped sidecar");
        assert_eq!(result.warnings.len(), 1, "expected one warning");
    }

    #[test]
    fn missing_required_token_field_is_a_diagnostic() {
        let dir = tmp_dir("missing-token");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.remove("input_tokens");
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn null_required_token_field_is_a_diagnostic() {
        let dir = tmp_dir("null-token");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.insert("output_tokens".into(), Value::Null);
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn string_token_field_is_a_diagnostic() {
        let dir = tmp_dir("string-token");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.insert("cache_read_tokens".into(), Value::from("12"));
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn negative_token_field_is_a_diagnostic() {
        let dir = tmp_dir("negative-token");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.insert("cache_write_tokens".into(), Value::from(-1));
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fractional_token_field_is_a_diagnostic() {
        let dir = tmp_dir("fractional-token");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.insert("input_tokens".into(), Value::from(1.5));
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn overflowing_token_field_is_a_diagnostic() {
        let dir = tmp_dir("overflow-token");
        let sessions = dir.join("sessions");
        let session_id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        let session_dir = sessions.join(session_id);
        fs::create_dir_all(&session_dir).unwrap();
        let raw = format!(
            r#"{{"schema_version":1,"session_id":"{session_id}","snapshot":{{"schema_version":1,"billing":"complete","total_cost":0.02,"models":[{{"model":"openai/gpt-5.4","total_cost":0.02,"input_tokens":18446744073709551616,"output_tokens":4,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"request_count":1}}]}}}}"#
        );
        fs::write(session_dir.join(SIDECAR_FILE), raw).unwrap();
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn blank_model_name_is_a_diagnostic() {
        let dir = tmp_dir("blank-model");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.insert("model".into(), Value::from("   "));
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_model_cell_is_a_diagnostic() {
        let dir = tmp_dir("missing-model");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.remove("model");
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_request_count_is_a_diagnostic() {
        let dir = tmp_dir("bad-request-count");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            snapshot_with_model(|m| {
                m.insert("request_count".into(), Value::from("many"));
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extra_top_level_key_is_a_diagnostic() {
        let dir = tmp_dir("extra-key");
        let sessions = dir.join("sessions");
        let session_id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        let session_dir = sessions.join(session_id);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": session_id,
            "snapshot": { "schema_version": 1, "billing": "complete", "models": [] },
            "extra": true,
        });
        fs::write(session_dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_snapshot_schema_version_is_a_diagnostic() {
        let dir = tmp_dir("no-inner-version");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            serde_json::json!({
                "billing": "complete",
                "models": []
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn session_id_directory_mismatch_is_a_diagnostic() {
        let dir = tmp_dir("dir-mismatch");
        let sessions = dir.join("sessions");
        let dir_name = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        let session_dir = sessions.join(dir_name);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": "1770000000000-1770000000000000000-b1b2c3d4e5f60718",
            "snapshot": { "schema_version": 1, "billing": "complete", "models": [] },
        });
        fs::write(session_dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absent_models_is_a_diagnostic() {
        let dir = tmp_dir("no-models");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete"
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_array_models_is_a_diagnostic() {
        let dir = tmp_dir("models-not-array");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "complete",
                "models": { "model": "openai/gpt-5.4" }
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_billing_is_a_diagnostic() {
        let dir = tmp_dir("unknown-billing");
        let sessions = dir.join("sessions");
        write_sidecar(
            &sessions,
            "1770000000000-1770000000000000000-a1b2c3d4e5f60718",
            serde_json::json!({
                "schema_version": 1,
                "billing": "mystery",
                "models": []
            }),
        );
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_session_dir_name_is_a_diagnostic() {
        let dir = tmp_dir("bad-dir-name");
        let sessions = dir.join("sessions");
        let dir_name = "not-a-valid-id";
        let session_dir = sessions.join(dir_name);
        fs::create_dir_all(&session_dir).unwrap();
        let envelope = serde_json::json!({
            "schema_version": 1,
            "session_id": dir_name,
            "snapshot": { "schema_version": 1, "billing": "complete", "models": [] },
        });
        fs::write(session_dir.join(SIDECAR_FILE), envelope.to_string()).unwrap();
        assert_only_diagnostic(&sessions);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nested_sidecar_is_ignored_and_does_not_shift_ordinals() {
        let dir = tmp_dir("nested");
        let sessions = dir.join("sessions");
        let session_id = "1770000000000-1770000000000000000-a1b2c3d4e5f60718";
        write_sidecar(&sessions, session_id, snapshot_with_model(|_| {}));
        let nested = sessions.join(session_id).join("deeper");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(SIDECAR_FILE), b"{not valid json").unwrap();
        let result = read_fx(&sessions);
        assert_eq!(result.events.len(), 1);
        assert!(result.skipped.is_empty(), "nested file must be ignored");
        assert!(result.warnings.is_empty());
        let expected = sessions
            .join(session_id)
            .join(SIDECAR_FILE)
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            result.events[0].message_id,
            format!("{session_id}:openai/gpt-5.4")
        );
        assert!(expected.ends_with("usage-v2.json"));
        fs::remove_dir_all(&dir).ok();
    }
}

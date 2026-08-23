use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, date_parse_ms, dir_exists, file_stem_name,
    finite_number, is_record, list_files, normalize_workspace, read_json,
};
use crate::types::{TokenCounts, UsageEvent};

/// The Reasonix reader wired into the harness registry. Reasonix (a Go
/// DeepSeek-native agent, `esengine/DeepSeek-Reasonix`) keeps its session
/// transcript in `<home>/projects/<slug>/sessions/<id>.jsonl`, but that
/// transcript and its `.events.jsonl` event log carry no token usage. The one
/// durable local usage store is the ACP metadata sidecar `<id>.acp.json`, which
/// records the session's model, working directory, and a cumulative usage total
/// (`status.cumulative`) for the whole session. The reader emits one event per
/// sidecar, deduped by session id (the latest `updatedAt` wins) so an
/// in-place-rewritten sidecar is never double counted. The home is `~/.reasonix`
/// on Unix and `%APPDATA%\reasonix` on Windows, overridable through
/// `REASONIX_HOME`.
pub struct ReasonixReader;

impl Reader for ReasonixReader {
    fn harness_id(&self) -> &'static str {
        "reasonix"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let home = reasonix_home(ctx.env("REASONIX_HOME"), ctx.env("APPDATA"), ctx.home());
        read_reasonix(&home)
    }
}

const HARNESS: &str = "reasonix";
const UNKNOWN_MODEL: &str = "reasonix-unknown";

/// Resolve the Reasonix home. `REASONIX_HOME` wins when set; otherwise, on
/// Windows, `%APPDATA%\reasonix` (`APPDATA` is only set on Windows, so falling
/// through to it is safe cross-platform); otherwise `~/.reasonix`.
pub fn reasonix_home(override_dir: Option<&str>, appdata: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    if let Some(v) = appdata
        && !v.is_empty()
    {
        return PathBuf::from(v).join("reasonix");
    }
    home.join(".reasonix")
}

/// A required token cell (`promptTokens`/`completionTokens` are always written
/// on a `status.cumulative`). The cell must be a non-negative finite number; an
/// absent, `null`, or invalid cell marks the sidecar malformed.
fn required_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// An optional token cell (`cacheHitTokens`/`cacheMissTokens`/`reasoningTokens`).
/// An absent or `null` cell is `0`; a populated cell that is not a non-negative
/// finite number marks the sidecar malformed.
fn optional_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// The outcome of parsing a `status.cumulative` usage map.
enum UsageOutcome {
    /// A populated, valid cumulative total with a non-zero token sum.
    Counts(TokenCounts),
    /// A valid cumulative total whose tokens are all zero (dropped, not warned).
    Zero,
    /// The cumulative map is absent/not an object, a cell is invalid, or the
    /// cumulative object is internally inconsistent (`reasoningTokens` exceeds
    /// `completionTokens`, which is impossible because completion includes
    /// reasoning).
    Malformed,
}

/// Parse a `status.cumulative` map into token counts. Reasonix reports the total
/// prompt in `promptTokens`, split into cached (`cacheHitTokens`) and uncached
/// (`cacheMissTokens`); the uncached portion (`cacheMissTokens`, default 0) is
/// the Skopli `input`. Completion tokens include reasoning, so `output`
/// subtracts `reasoningTokens`; a sidecar whose `reasoningTokens` exceeds
/// `completionTokens` is internally inconsistent and is treated as malformed.
/// Reasonix persists no cache-write cell, so `cacheWrite` is always 0.
fn parse_cumulative(cumulative: &Value) -> UsageOutcome {
    if !is_record(cumulative) {
        return UsageOutcome::Malformed;
    }
    let cells = [
        required_cell(cumulative.get("promptTokens")),
        required_cell(cumulative.get("completionTokens")),
        optional_cell(cumulative.get("cacheHitTokens")),
        optional_cell(cumulative.get("cacheMissTokens")),
        optional_cell(cumulative.get("reasoningTokens")),
    ];
    let [_prompt, completion, cache_hit, cache_miss, reasoning] = match cells {
        [Some(p), Some(c), Some(ch), Some(cm), Some(r)] => [p, c, ch, cm, r],
        _ => return UsageOutcome::Malformed,
    };
    // `completionTokens` includes `reasoningTokens`, so reasoning exceeding
    // completion is an internally inconsistent (corrupt or schema-drifted)
    // cumulative total, not a valid zero-output row.
    if reasoning > completion {
        return UsageOutcome::Malformed;
    }
    // `cacheMissTokens` is the uncached prompt and the Skopli `input`; an absent
    // hit/miss split defaults it to 0.
    let input = cache_miss;
    let output = completion - reasoning;
    if input == 0 && output == 0 && cache_hit == 0 && reasoning == 0 {
        return UsageOutcome::Zero;
    }
    UsageOutcome::Counts(TokenCounts {
        input,
        output,
        cache_read: cache_hit,
        cache_write: 0,
        cache_write1h: None,
        reasoning,
    })
}

/// A parsed sidecar candidate awaiting dedup by session id. `updated_at_ms` is
/// the `updatedAt` snapshot instant parsed once to epoch millis; it drives both
/// the emitted event timestamp and the latest-snapshot dedup ordering, so
/// selection and attribution use the same clock.
struct Candidate {
    event: UsageEvent,
    updated_at_ms: i64,
}

/// Parse one `<id>.acp.json` sidecar into a candidate event. A sidecar with no
/// `status.cumulative` object, or a zero-token total, yields `None` without a
/// warning; a populated-but-invalid cumulative is reported as malformed.
fn read_sidecar(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Option<Candidate> {
    let parsed = match read_json(file) {
        Some(v) if is_record(&v) => v,
        _ => {
            let loc = file.to_owned();
            warnings.push(ReaderWarning {
                message: format!("skipping malformed {HARNESS} file {loc}\n"),
            });
            skipped.push(loc);
            return None;
        }
    };
    let cumulative = parsed.get("status").and_then(|s| s.get("cumulative"))?;
    let tokens = match parse_cumulative(cumulative) {
        UsageOutcome::Counts(t) => t,
        UsageOutcome::Zero => return None,
        UsageOutcome::Malformed => {
            let loc = file.to_owned();
            warnings.push(ReaderWarning {
                message: format!("skipping malformed {HARNESS} file {loc}\n"),
            });
            skipped.push(loc);
            return None;
        }
    };
    // `updatedAt` is the snapshot timestamp; it must parse to a real instant so
    // that the same normalized value drives both the emitted timestamp and the
    // dedup ordering. A missing or unparseable `updatedAt` is malformed.
    let updated_at_ms = match parsed.get("updatedAt").and_then(as_string) {
        Some(raw) => match date_parse_ms(raw.trim()) {
            Some(ms) => ms,
            None => {
                let loc = file.to_owned();
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed {HARNESS} file {loc}\n"),
                });
                skipped.push(loc);
                return None;
            }
        },
        None => {
            let loc = file.to_owned();
            warnings.push(ReaderWarning {
                message: format!("skipping malformed {HARNESS} file {loc}\n"),
            });
            skipped.push(loc);
            return None;
        }
    };
    let timestamp = match crate::time::to_iso_string(updated_at_ms) {
        Some(ts) => ts,
        None => {
            let loc = file.to_owned();
            warnings.push(ReaderWarning {
                message: format!("skipping malformed {HARNESS} file {loc}\n"),
            });
            skipped.push(loc);
            return None;
        }
    };
    let session_id = parsed
        .get("sessionId")
        .and_then(as_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| file_stem_name(file));
    let model = parsed
        .get("model")
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| UNKNOWN_MODEL.to_owned());
    let workspace = parsed.get("cwd").and_then(normalize_workspace);
    let title = parsed
        .get("title")
        .and_then(as_string)
        .filter(|t| !t.trim().is_empty());
    Some(Candidate {
        event: UsageEvent {
            harness: HARNESS.to_owned(),
            timestamp,
            session_id,
            message_id: file.to_owned(),
            turn: true,
            subagent: false,
            model,
            tokens,
            calls: None,
            cost_usd: None,
            workspace,
            title,
        },
        updated_at_ms,
    })
}

/// Read all Reasonix usage events reachable from `home`. Walks
/// `<home>/projects/*/sessions/*.acp.json`, emitting one cumulative event per
/// session id. When two sidecars share a session id (e.g. a recovery-branch
/// twin), the one with the later `updatedAt` wins.
pub fn read_reasonix(home: &Path) -> ReaderResult {
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let projects = home.join("projects");
    if !dir_exists(&projects) {
        return ReaderResult {
            events: Vec::new(),
            skipped,
            warnings,
        };
    }

    let files = list_files(&projects, |name| name.ends_with(".acp.json"));
    let mut by_session: HashMap<String, Candidate> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for file in &files {
        let candidate = match read_sidecar(file, &mut skipped, &mut warnings) {
            Some(c) => c,
            None => continue,
        };
        let key = candidate.event.session_id.clone();
        match by_session.get(&key) {
            Some(existing) if existing.updated_at_ms >= candidate.updated_at_ms => {}
            Some(_) => {
                by_session.insert(key, candidate);
            }
            None => {
                order.push(key.clone());
                by_session.insert(key, candidate);
            }
        }
    }

    let events = order
        .into_iter()
        .filter_map(|k| by_session.remove(&k).map(|c| c.event))
        .collect();

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
    fn default_home_resolves_dot_reasonix() {
        assert_eq!(reasonix_home(None, None, &home()), home().join(".reasonix"));
    }

    #[test]
    fn reasonix_home_env_override_wins() {
        assert_eq!(
            reasonix_home(
                Some("/custom/rx"),
                Some("C:\\Users\\u\\AppData\\Roaming"),
                &home()
            ),
            PathBuf::from("/custom/rx")
        );
    }

    #[test]
    fn appdata_selects_windows_home() {
        assert_eq!(
            reasonix_home(None, Some("C:\\Users\\u\\AppData\\Roaming"), &home()),
            PathBuf::from("C:\\Users\\u\\AppData\\Roaming").join("reasonix")
        );
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-reasonix-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn write_sidecar(dir: &Path, slug: &str, id: &str, body: &str) -> PathBuf {
        let sessions = dir.join("projects").join(slug).join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join(format!("{id}.acp.json"));
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn empty_home_yields_no_events_then_reads_after_sidecar_appears() {
        let dir = tmp_dir("presence");
        assert!(read_reasonix(&dir).events.is_empty());
        write_sidecar(
            &dir,
            "-home-u-proj",
            "sess-a",
            r#"{"sessionId":"sess-a","cwd":"/home/u/proj","model":"deepseek-chat","updatedAt":"2026-08-01T10:00:00Z","status":{"cumulative":{"promptTokens":100,"completionTokens":50,"cacheHitTokens":30,"cacheMissTokens":70,"reasoningTokens":10}}}"#,
        );
        let events = read_reasonix(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, "sess-a");
        assert_eq!(events[0].model, "deepseek-chat");
        assert_eq!(events[0].timestamp, "2026-08-01T10:00:00.000Z");
        assert_eq!(events[0].tokens.input, 70, "input = cacheMissTokens");
        assert_eq!(
            events[0].tokens.output, 40,
            "output = completion - reasoning"
        );
        assert_eq!(events[0].tokens.cache_read, 30);
        assert_eq!(events[0].tokens.cache_write, 0);
        assert_eq!(events[0].tokens.reasoning, 10);
        assert_eq!(events[0].workspace.as_deref(), Some("/home/u/proj"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absent_cache_split_yields_zero_input() {
        let dir = tmp_dir("no-split");
        write_sidecar(
            &dir,
            "-p",
            "s",
            r#"{"sessionId":"s","model":"m","updatedAt":"2026-08-01T10:00:00Z","status":{"cumulative":{"promptTokens":80,"completionTokens":20}}}"#,
        );
        let events = read_reasonix(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].tokens.input, 0,
            "input = cacheMissTokens (default 0), not promptTokens"
        );
        assert_eq!(events[0].tokens.output, 20);
        assert_eq!(events[0].tokens.cache_read, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn estimated_sessions_are_included() {
        let dir = tmp_dir("estimated");
        write_sidecar(
            &dir,
            "-p",
            "s",
            r#"{"sessionId":"s","model":"m","updatedAt":"2026-08-01T10:00:00Z","status":{"cumulative":{"promptTokens":10,"completionTokens":5,"cacheMissTokens":10,"estimated":true}}}"#,
        );
        let events = read_reasonix(&dir).events;
        assert_eq!(events.len(), 1, "estimated sessions still emit real tokens");
        assert_eq!(events[0].tokens.input, 10);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reasoning_exceeding_completion_is_malformed() {
        let dir = tmp_dir("reasoning-gt-completion");
        let file = write_sidecar(
            &dir,
            "-p",
            "s",
            r#"{"sessionId":"s","model":"m","updatedAt":"2026-08-01T10:00:00Z","status":{"cumulative":{"promptTokens":100,"completionTokens":30,"cacheMissTokens":50,"reasoningTokens":40}}}"#,
        );
        let result = read_reasonix(&dir);
        assert!(
            result.events.is_empty(),
            "inconsistent cumulative is skipped"
        );
        let loc = file.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![loc.clone()]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed reasonix file {loc}\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn latest_updated_at_wins_for_same_session_id() {
        let dir = tmp_dir("dedup");
        write_sidecar(
            &dir,
            "-p",
            "s-old",
            r#"{"sessionId":"dup","model":"m","updatedAt":"2026-08-01T09:00:00Z","status":{"cumulative":{"promptTokens":1,"completionTokens":1,"cacheMissTokens":1}}}"#,
        );
        write_sidecar(
            &dir,
            "-p",
            "s-new",
            r#"{"sessionId":"dup","model":"m","updatedAt":"2026-08-01T11:00:00Z","status":{"cumulative":{"promptTokens":100,"completionTokens":100,"cacheMissTokens":100}}}"#,
        );
        let events = read_reasonix(&dir).events;
        assert_eq!(events.len(), 1, "one event per session id");
        assert_eq!(events[0].tokens.input, 100, "latest updatedAt wins");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dedup_orders_by_instant_not_lexicographic_string() {
        // `2026-08-03T09:00:00+02:00` (07:00:00Z) is chronologically EARLIER
        // than `2026-08-03T08:30:00Z`, yet its raw string sorts LATER as text.
        // The winner must be the later instant (the `Z` snapshot).
        let dir = tmp_dir("dedup-offset");
        write_sidecar(
            &dir,
            "-p",
            "s-offset",
            r#"{"sessionId":"dup","model":"m","updatedAt":"2026-08-03T09:00:00+02:00","status":{"cumulative":{"promptTokens":1,"completionTokens":1,"cacheMissTokens":1}}}"#,
        );
        write_sidecar(
            &dir,
            "-p",
            "s-zulu",
            r#"{"sessionId":"dup","model":"m","updatedAt":"2026-08-03T08:30:00Z","status":{"cumulative":{"promptTokens":100,"completionTokens":100,"cacheMissTokens":100}}}"#,
        );
        let events = read_reasonix(&dir).events;
        assert_eq!(events.len(), 1, "one event per session id");
        assert_eq!(
            events[0].tokens.input, 100,
            "later instant wins even though its string sorts earlier"
        );
        assert_eq!(
            events[0].timestamp, "2026-08-03T08:30:00.000Z",
            "emitted timestamp is the same normalized instant used for dedup"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_or_malformed_updated_at_is_malformed() {
        let dir = tmp_dir("bad-updated-at");
        let missing = write_sidecar(
            &dir,
            "-p",
            "s-missing",
            r#"{"sessionId":"a","model":"m","status":{"cumulative":{"promptTokens":10,"completionTokens":5,"cacheMissTokens":10}}}"#,
        );
        let malformed = write_sidecar(
            &dir,
            "-p",
            "s-malformed",
            r#"{"sessionId":"b","model":"m","updatedAt":"not-a-date","status":{"cumulative":{"promptTokens":10,"completionTokens":5,"cacheMissTokens":10}}}"#,
        );
        let result = read_reasonix(&dir);
        assert!(
            result.events.is_empty(),
            "no snapshot timestamp = malformed"
        );
        let mut skipped = result.skipped.clone();
        skipped.sort();
        let mut expected = vec![
            missing.to_string_lossy().into_owned(),
            malformed.to_string_lossy().into_owned(),
        ];
        expected.sort();
        assert_eq!(skipped, expected);
        for w in &result.warnings {
            assert!(w.message.starts_with("skipping malformed reasonix file "));
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sidecar_without_status_yields_nothing() {
        let dir = tmp_dir("no-status");
        write_sidecar(
            &dir,
            "-p",
            "s",
            r#"{"sessionId":"s","model":"m","updatedAt":"2026-08-01T10:00:00Z"}"#,
        );
        let result = read_reasonix(&dir);
        assert!(result.events.is_empty());
        assert!(result.warnings.is_empty(), "absent status is not malformed");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zero_token_total_is_dropped() {
        let dir = tmp_dir("zero");
        write_sidecar(
            &dir,
            "-p",
            "s",
            r#"{"sessionId":"s","model":"m","updatedAt":"2026-08-01T10:00:00Z","status":{"cumulative":{"promptTokens":0,"completionTokens":0}}}"#,
        );
        assert!(read_reasonix(&dir).events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_required_cell_is_malformed() {
        let dir = tmp_dir("required");
        let file = write_sidecar(
            &dir,
            "-p",
            "s",
            r#"{"sessionId":"s","model":"m","updatedAt":"2026-08-01T10:00:00Z","status":{"cumulative":{"completionTokens":5}}}"#,
        );
        let result = read_reasonix(&dir);
        assert!(result.events.is_empty());
        let loc = file.to_string_lossy().into_owned();
        assert_eq!(result.skipped, vec![loc.clone()]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed reasonix file {loc}\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transcript_jsonl_is_not_read() {
        let dir = tmp_dir("transcript");
        let sessions = dir.join("projects").join("-p").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("s.jsonl"),
            "{\"role\":\"assistant\",\"content\":\"hi\"}\n",
        )
        .unwrap();
        fs::write(
            sessions.join("s.events.jsonl"),
            "{\"schema_version\":1,\"type\":\"append\"}\n",
        )
        .unwrap();
        assert!(
            read_reasonix(&dir).events.is_empty(),
            "only .acp.json is read"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

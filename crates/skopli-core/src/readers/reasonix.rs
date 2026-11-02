use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, date_parse_ms, dir_exists, file_stem_name, is_record,
    list_files, normalize_workspace, optional_cell, parse_timestamp_to_iso, read_json,
    read_jsonl_lines, required_cell,
};
use crate::types::{TokenCounts, UsageEvent};

/// The Reasonix reader wired into the harness registry. Reasonix (a Go
/// DeepSeek-native agent, `esengine/DeepSeek-Reasonix`) keeps its session
/// transcript in `<home>/projects/<slug>/sessions/<id>.jsonl`, but that
/// transcript and its `.events.jsonl` event log carry no token usage. Current
/// builds append one row per request to the `<home>/stats/<date>.jsonl` ledger
/// (`cache_miss` + `cache_hit` = `prompt`, `completion`, optional `reasoning`),
/// which is the one per-request store and is read when present, one event per
/// row keyed by the ledger's day. Without it the durable usage store is the
/// per-session sidecar (`<id>.acp.json`, or `<id>.jsonl.telemetry.json` +
/// `<id>.jsonl.meta`), whose cumulative total the reader emits as one event per
/// session id (the latest `updatedAt` wins) so an in-place-rewritten sidecar is
/// never double counted. The home is `~/.reasonix` on Unix and
/// `%APPDATA%\reasonix` on Windows, overridable through `REASONIX_HOME`.
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
            skip_malformed(file, skipped, warnings);
            return None;
        }
    };
    let cumulative = parsed.get("status").and_then(|s| s.get("cumulative"))?;
    let tokens = match parse_cumulative(cumulative) {
        UsageOutcome::Counts(t) => t,
        UsageOutcome::Zero => return None,
        UsageOutcome::Malformed => {
            skip_malformed(file, skipped, warnings);
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
                skip_malformed(file, skipped, warnings);
                return None;
            }
        },
        None => {
            skip_malformed(file, skipped, warnings);
            return None;
        }
    };
    let timestamp = match crate::time::to_iso_string(updated_at_ms) {
        Some(ts) => ts,
        None => {
            skip_malformed(file, skipped, warnings);
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

const TELEMETRY_SUFFIX: &str = ".jsonl.telemetry.json";
const META_SUFFIX: &str = ".jsonl.meta";

fn skip_malformed(file: &str, skipped: &mut Vec<String>, warnings: &mut Vec<ReaderWarning>) {
    warnings.push(ReaderWarning {
        message: format!("skipping malformed {HARNESS} file {file}\n"),
    });
    skipped.push(file.to_owned());
}

/// Reasonix 2.56 replaced the `.acp.json` sidecar with two files beside the
/// transcript: `<id>.jsonl.telemetry.json` (`usage` carries the same cells as
/// `status.cumulative`) and `<id>.jsonl.meta` (`id`, `model`, `updated_at`,
/// `preview`). The pair is one candidate; a telemetry file whose meta is
/// missing or unparseable is malformed.
fn read_telemetry(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Option<Candidate> {
    let telemetry = match read_json(file) {
        Some(v) if is_record(&v) => v,
        _ => {
            skip_malformed(file, skipped, warnings);
            return None;
        }
    };
    let usage = telemetry.get("usage")?;
    let tokens = match parse_cumulative(usage) {
        UsageOutcome::Counts(t) => t,
        UsageOutcome::Zero => return None,
        UsageOutcome::Malformed => {
            skip_malformed(file, skipped, warnings);
            return None;
        }
    };
    let stem = file.strip_suffix(TELEMETRY_SUFFIX).unwrap_or(file);
    let meta = match read_json(&format!("{stem}{META_SUFFIX}")) {
        Some(v) if is_record(&v) => v,
        _ => {
            skip_malformed(file, skipped, warnings);
            return None;
        }
    };
    let updated_at_ms = meta
        .get("updated_at")
        .and_then(as_string)
        .and_then(|raw| date_parse_ms(raw.trim()));
    let (updated_at_ms, timestamp) =
        match updated_at_ms.and_then(|ms| crate::time::to_iso_string(ms).map(|ts| (ms, ts))) {
            Some(pair) => pair,
            None => {
                skip_malformed(file, skipped, warnings);
                return None;
            }
        };
    let session_id = meta
        .get("id")
        .and_then(as_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| super::shared::basename(stem));
    let model = meta
        .get("model")
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| UNKNOWN_MODEL.to_owned());
    let title = meta
        .get("preview")
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
            workspace: None,
            title,
        },
        updated_at_ms,
    })
}

/// One event per usage row of a `<home>/stats/<date>.jsonl` ledger. A row is a
/// request (`prompt` = `cache_hit` + `cache_miss`, `completion`, optional
/// `reasoning` inside the completion); `{"turn":true}` rows are turn markers
/// and carry nothing. The rows name no session, so the file date is the session
/// id and the row location is the message id.
fn read_stats(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let mut events = Vec::new();
    let Some(lines) = read_jsonl_lines(file, skipped, warnings) else {
        return events;
    };
    let session_id = file_stem_name(file);
    for line in &lines {
        let row = &line.value;
        if row.get("turn").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let loc = format!("{file}:{}", line.index + 1);
        let cells = [
            required_cell(row.get("cache_miss")),
            required_cell(row.get("cache_hit")),
            required_cell(row.get("completion")),
            optional_cell(row.get("reasoning")),
        ];
        let timestamp = row.get("ts").and_then(parse_timestamp_to_iso);
        let (
            [
                Some(input),
                Some(cache_read),
                Some(completion),
                Some(reasoning),
            ],
            Some(timestamp),
        ) = (cells, timestamp)
        else {
            skip_malformed(&loc, skipped, warnings);
            continue;
        };
        events.push(UsageEvent {
            harness: HARNESS.to_owned(),
            timestamp,
            session_id: session_id.clone(),
            message_id: loc,
            turn: true,
            subagent: false,
            model: row
                .get("model")
                .and_then(as_string)
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| UNKNOWN_MODEL.to_owned()),
            tokens: TokenCounts {
                input,
                output: completion.saturating_sub(reasoning),
                cache_read,
                cache_write: 0,
                cache_write1h: None,
                reasoning,
            },
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        });
    }
    events
}

/// Read all Reasonix usage events reachable from `home`. A home with a
/// `stats/` ledger is read from that ledger alone (one event per request), as
/// the project sidecars of such a build hold only each session's last request.
/// Otherwise the reader walks
/// `<home>/projects/*/sessions/`, reading `*.acp.json` sidecars and the
/// `*.jsonl.telemetry.json` + `*.jsonl.meta` pairs newer builds write, emitting
/// one cumulative event per session id. When two sidecars share a session id
/// (e.g. a recovery-branch twin), the one with the later `updatedAt` wins.
pub fn read_reasonix(home: &Path) -> ReaderResult {
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let stats = home.join("stats");
    if dir_exists(&stats) {
        let mut events = Vec::new();
        for file in list_files(&stats, |name| name.ends_with(".jsonl")) {
            events.extend(read_stats(&file, &mut skipped, &mut warnings));
        }
        return ReaderResult {
            events,
            skipped,
            warnings,
        };
    }
    let projects = home.join("projects");
    if !dir_exists(&projects) {
        return ReaderResult {
            events: Vec::new(),
            skipped,
            warnings,
        };
    }

    let files = list_files(&projects, |name| {
        name.ends_with(".acp.json") || name.ends_with(TELEMETRY_SUFFIX)
    });
    let mut by_session: HashMap<String, Candidate> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for file in &files {
        let candidate = match if file.ends_with(TELEMETRY_SUFFIX) {
            read_telemetry(file, &mut skipped, &mut warnings)
        } else {
            read_sidecar(file, &mut skipped, &mut warnings)
        } {
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
    use crate::readers::shared::{scratch_dir, test_home};
    use std::fs;

    const STATS: &str = concat!(
        r#"{"ts":"2026-10-25T12:23:06.9936797Z","model":"qwen/qwen","source":"cli","prompt":6206,"completion":62,"cache_hit":3,"cache_miss":6203,"total":6268,"requests":1}"#,
        "\n",
        r#"{"ts":"2026-10-25T12:23:07.007497757Z","source":"cli","turn":true}"#,
        "\n",
        r#"{"ts":"2026-10-25T12:23:12.937156518Z","model":"qwen/qwen","source":"cli","prompt":6940,"completion":15,"cache_hit":6267,"cache_miss":673,"total":6955,"requests":1,"reasoning":5}"#,
        "\n"
    );

    fn stats_home(rows: &str) -> PathBuf {
        let home = scratch_dir("reasonix-stats");
        fs::create_dir_all(home.join("stats")).unwrap();
        fs::write(home.join("stats").join("2026-10-25.jsonl"), rows).unwrap();
        home
    }

    #[test]
    fn stats_ledger_yields_one_event_per_request() {
        let home = stats_home(STATS);
        let r = read_reasonix(&home);
        assert_eq!(r.events.len(), 2);
        let e = &r.events[0];
        assert_eq!(e.session_id, "2026-10-25");
        assert_eq!(e.timestamp, "2026-10-25T12:23:06.993Z");
        assert_eq!(e.model, "qwen/qwen");
        assert_eq!(
            (e.tokens.input, e.tokens.cache_read, e.tokens.output),
            (6203, 3, 62)
        );
        assert_eq!(
            (r.events[1].tokens.output, r.events[1].tokens.reasoning),
            (10, 5)
        );
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn stats_ledger_wins_over_project_sidecars() {
        let home = stats_home(STATS);
        let sessions = home.join("projects").join("p").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("s1.acp.json"),
            r#"{"sessionId":"s1","model":"m","updatedAt":"2026-10-25T12:30:00Z","status":{"cumulative":{"promptTokens":99,"completionTokens":9,"cacheHitTokens":0,"cacheMissTokens":99}}}"#,
        )
        .unwrap();
        let r = read_reasonix(&home);
        assert_eq!(r.events.len(), 2);
        assert!(r.events.iter().all(|e| e.session_id == "2026-10-25"));
    }

    #[test]
    fn stats_row_with_bad_cell_is_skipped_with_a_warning() {
        let home = stats_home(&STATS.replace("\"cache_miss\":673", "\"cache_miss\":\"x\""));
        let r = read_reasonix(&home);
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].ends_with("2026-10-25.jsonl:3"));
    }

    #[test]
    fn default_home_resolves_dot_reasonix() {
        assert_eq!(
            reasonix_home(None, None, &test_home()),
            test_home().join(".reasonix")
        );
    }

    #[test]
    fn reasonix_home_env_override_wins() {
        assert_eq!(
            reasonix_home(
                Some("/custom/rx"),
                Some("C:\\Users\\u\\AppData\\Roaming"),
                &test_home()
            ),
            PathBuf::from("/custom/rx")
        );
    }

    #[test]
    fn appdata_selects_windows_home() {
        assert_eq!(
            reasonix_home(None, Some("C:\\Users\\u\\AppData\\Roaming"), &test_home()),
            PathBuf::from("C:\\Users\\u\\AppData\\Roaming").join("reasonix")
        );
    }

    fn write_sidecar(dir: &Path, slug: &str, id: &str, body: &str) -> PathBuf {
        let sessions = dir.join("projects").join(slug).join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join(format!("{id}.acp.json"));
        fs::write(&path, body).unwrap();
        path
    }

    fn write_pair(dir: &Path, slug: &str, id: &str, telemetry: &str, meta: Option<&str>) {
        let sessions = dir.join("projects").join(slug).join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join(format!("{id}.jsonl.telemetry.json")),
            telemetry,
        )
        .unwrap();
        if let Some(meta) = meta {
            fs::write(sessions.join(format!("{id}.jsonl.meta")), meta).unwrap();
        }
    }

    // The shape Reasonix 2.56.1 wrote on Windows: usage in the telemetry
    // sidecar, identity and timestamps in the meta file, no .acp.json.
    #[test]
    fn telemetry_and_meta_pair_yields_one_event() {
        let dir = scratch_dir("telemetry");
        write_pair(
            &dir,
            "c--users-u-proj",
            "20261101-211432.883022900-qwen",
            r#"{"version":1,"usage":{"promptTokens":7477,"completionTokens":30,"reasoningTokens":0,"cacheHitTokens":3,"cacheMissTokens":7474,"requestCount":1}}"#,
            Some(
                r#"{"id":"20261101-211432.883022900-qwen","created_at":"2026-11-01T21:14:32.9124106Z","updated_at":"2026-11-01T21:15:03.8429271Z","model":"llama/qwen","turns":1,"preview":"What does index.js export?"}"#,
            ),
        );
        let result = read_reasonix(&dir);
        assert!(result.warnings.is_empty());
        assert_eq!(result.events.len(), 1);
        let e = &result.events[0];
        assert_eq!(e.session_id, "20261101-211432.883022900-qwen");
        assert_eq!(e.model, "llama/qwen");
        assert_eq!(e.timestamp, "2026-11-01T21:15:03.842Z");
        assert_eq!(e.tokens.input, 7474);
        assert_eq!(e.tokens.output, 30);
        assert_eq!(e.tokens.cache_read, 3);
        assert_eq!(e.tokens.cache_write, 0);
        assert_eq!(e.title.as_deref(), Some("What does index.js export?"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn telemetry_without_meta_is_malformed() {
        let dir = scratch_dir("telemetry-no-meta");
        write_pair(
            &dir,
            "p",
            "s1",
            r#"{"version":1,"usage":{"promptTokens":10,"completionTokens":5,"cacheMissTokens":10}}"#,
            None,
        );
        let result = read_reasonix(&dir);
        assert!(result.events.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn telemetry_and_acp_for_one_session_keep_the_latest() {
        let dir = scratch_dir("telemetry-vs-acp");
        write_sidecar(
            &dir,
            "p",
            "s1",
            r#"{"sessionId":"s1","model":"old","updatedAt":"2026-08-01T10:00:00Z","status":{"cumulative":{"promptTokens":100,"completionTokens":50,"cacheMissTokens":100}}}"#,
        );
        write_pair(
            &dir,
            "p",
            "s1",
            r#"{"version":1,"usage":{"promptTokens":10,"completionTokens":5,"cacheMissTokens":10}}"#,
            Some(r#"{"id":"s1","model":"new","updated_at":"2026-08-02T10:00:00Z"}"#),
        );
        let events = read_reasonix(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "new");
        assert_eq!(events[0].tokens.input, 10);
        fs::remove_dir_all(&dir).ok();
    }
    #[test]
    fn empty_home_yields_no_events_then_reads_after_sidecar_appears() {
        let dir = scratch_dir("presence");
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
        let dir = scratch_dir("no-split");
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
        let dir = scratch_dir("estimated");
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
        let dir = scratch_dir("reasoning-gt-completion");
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
        let dir = scratch_dir("dedup");
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
        let dir = scratch_dir("dedup-offset");
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
        let dir = scratch_dir("bad-updated-at");
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
        let dir = scratch_dir("no-status");
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
        let dir = scratch_dir("zero");
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
        let dir = scratch_dir("required");
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
        let dir = scratch_dir("transcript");
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

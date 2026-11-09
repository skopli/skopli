use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, date_parse_ms, dir_exists, file_mtime_iso,
    file_stem_name, is_record, optional_cell, parse_timestamp_to_iso, read_json, required_cell,
};
use crate::time::to_iso_string;
use crate::types::{TokenCounts, UsageEvent};

/// The trae-agent reader wired into the harness registry. trae-agent
/// (`bytedance/trae-agent`) writes one trajectory JSON per run under
/// `<cwd>/trajectories/trajectory_<YYYYMMDD_HHMMSS>.json` -- a project-local
/// path relative to the working directory, with no home or environment anchor.
/// Each trajectory records every model call twice: once in `llm_interactions`
/// (the full record, carrying the cache-split usage) and again, partially, in
/// `agent_steps[].llm_response`. The reader emits one event per
/// `llm_interactions` entry and never reads `agent_steps`, so a call is never
/// double counted. When the context carries no working directory the reader
/// yields nothing rather than erroring.
///
/// trae stamps every record with `datetime.now().isoformat()`: local wall time
/// with no offset. The trajectory is rewritten right after each stamp is taken
/// (`end_time` last), so the file mtime is the UTC instant of its newest
/// timestamp; the gap between the two, rounded to the 15-minute grid every real
/// zone sits on, is the local offset and is applied to each naive timestamp. A
/// file whose mtime no longer tracks its content (a copy, a checkout) has no
/// usable anchor: its naive timestamps are booked as UTC and one warning names
/// the file.
pub struct TraeReader;

impl Reader for TraeReader {
    fn harness_id(&self) -> &'static str {
        "trae"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        match ctx.cwd() {
            Some(cwd) => read_trae(cwd),
            None => ReaderResult::default(),
        }
    }
}

const HARNESS: &str = "trae";
const UNKNOWN_MODEL: &str = "trae-unknown";

/// The trajectories root: `<cwd>/trajectories`.
pub fn trae_trajectories_root(cwd: &Path) -> PathBuf {
    cwd.join("trajectories")
}

/// The outcome of parsing one interaction `response.usage` map.
enum UsageOutcome {
    /// A populated, valid usage map with a non-zero total.
    Counts(TokenCounts),
    /// A valid usage map whose token counts are all zero (dropped, not warned).
    Zero,
    /// The usage map is absent or not an object, or a populated cell is invalid.
    Malformed,
}

/// Per-interaction usage. `input_tokens`/`output_tokens` are the required cells;
/// `cache_creation_input_tokens` (cache write), `cache_read_input_tokens`
/// (cache read), and `reasoning_tokens` may be `null` (treated as 0). A usage
/// map that is absent or not an object, or one of whose populated cells is not a
/// non-negative number, is `Malformed`. An all-zero usage map is `Zero`.
fn parse_usage(response: &Value) -> UsageOutcome {
    let usage = match response.get("usage") {
        Some(u) if is_record(u) => u,
        _ => return UsageOutcome::Malformed,
    };
    let cells = [
        required_cell(usage.get("input_tokens")),
        required_cell(usage.get("output_tokens")),
        optional_cell(usage.get("cache_read_input_tokens")),
        optional_cell(usage.get("cache_creation_input_tokens")),
        optional_cell(usage.get("reasoning_tokens")),
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

/// Read one trajectory file into usage events, one per `llm_interactions` entry.
/// The session id is the file stem; the model prefers the response model, then
/// the interaction model, then the trajectory's top-level model; the timestamp
/// prefers the interaction timestamp, then the trajectory `start_time`, then the
/// file mtime. An unreadable or invalid trajectory file (non-object root or
/// non-array `llm_interactions`) is skipped with a file-level diagnostic; a
/// malformed interaction is skipped with a per-interaction diagnostic.
fn read_trajectory(
    file: &str,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
) -> Vec<UsageEvent> {
    let mut events: Vec<UsageEvent> = Vec::new();
    let parsed = match read_json(file) {
        Some(v) if is_record(&v) => v,
        Some(_) => {
            warnings.push(ReaderWarning {
                message: format!("skipping malformed trae trajectory {file}\n"),
            });
            skipped.push(file.to_owned());
            return events;
        }
        None => {
            warnings.push(ReaderWarning {
                message: format!("skipping unreadable trae trajectory {file}\n"),
            });
            skipped.push(file.to_owned());
            return events;
        }
    };
    let interactions = match parsed.get("llm_interactions") {
        Some(Value::Array(a)) => a,
        _ => {
            warnings.push(ReaderWarning {
                message: format!("skipping malformed trae trajectory {file}\n"),
            });
            skipped.push(file.to_owned());
            return events;
        }
    };

    let session_id = file_stem_name(file);
    let offset_ms = naive_offset_ms(&parsed, interactions, file).unwrap_or_else(|| {
        warnings.push(ReaderWarning {
            message: format!("naive timestamps, no usable anchor in {file}\n"),
        });
        0
    });
    let stamp = |value: &Value| -> Option<String> {
        let iso = parse_timestamp_to_iso(value)?;
        let shifted = naive_ms(value).and_then(|ms| to_iso_string(ms.checked_add(offset_ms)?));
        Some(shifted.unwrap_or(iso))
    };
    let fallback_timestamp = parsed
        .get("start_time")
        .and_then(stamp)
        .unwrap_or_else(|| file_mtime_iso(file));
    let trajectory_model = parsed
        .get("model")
        .and_then(as_string)
        .filter(|m| !m.trim().is_empty());

    for (index, interaction) in interactions.iter().enumerate() {
        // Skipped/warning labels use a one-based ordinal (index + 1) so the
        // `<file>:<n>` diagnostic shape matches every other reader; the opaque
        // message_id below stays zero-based because it is an identity, not a
        // displayed location.
        let ordinal = index + 1;
        if !is_record(interaction) {
            let loc = format!("{file}:{ordinal}");
            warnings.push(ReaderWarning {
                message: format!("skipping malformed trae interaction {loc}\n"),
            });
            skipped.push(loc);
            continue;
        }
        let response = match interaction.get("response") {
            Some(r) if is_record(r) => r,
            _ => {
                let loc = format!("{file}:{ordinal}");
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed trae interaction {loc}\n"),
                });
                skipped.push(loc);
                continue;
            }
        };
        let tokens = match parse_usage(response) {
            UsageOutcome::Counts(t) => t,
            UsageOutcome::Zero => continue,
            UsageOutcome::Malformed => {
                let loc = format!("{file}:{ordinal}");
                warnings.push(ReaderWarning {
                    message: format!("skipping malformed trae interaction {loc}\n"),
                });
                skipped.push(loc);
                continue;
            }
        };
        let model = response
            .get("model")
            .and_then(as_string)
            .filter(|m| !m.trim().is_empty())
            .or_else(|| {
                interaction
                    .get("model")
                    .and_then(as_string)
                    .filter(|m| !m.trim().is_empty())
            })
            .or_else(|| trajectory_model.clone())
            .unwrap_or_else(|| UNKNOWN_MODEL.to_owned());
        let timestamp = interaction
            .get("timestamp")
            .and_then(stamp)
            .unwrap_or_else(|| fallback_timestamp.clone());
        events.push(UsageEvent {
            harness: HARNESS.to_owned(),
            timestamp,
            session_id: session_id.clone(),
            message_id: format!("{file}:{index}"),
            turn: true,
            subagent: false,
            model,
            tokens,
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        });
    }
    events
}

const QUARTER_HOUR_MS: i64 = 15 * 60 * 1000;
const MAX_OFFSET_MS: i64 = 14 * 60 * 60 * 1000;
/// trae saves within milliseconds of taking a stamp; a copy or checkout lands
/// this close to the grid by chance only two minutes in fifteen.
const MAX_RESIDUAL_MS: i64 = 2 * 60 * 1000;

/// Epoch millis of a date-time string that carries neither `Z` nor a numeric
/// offset; `None` for anything else (offset-aware, numeric, absent).
fn naive_ms(value: &Value) -> Option<i64> {
    let raw = value.as_str()?.trim();
    let time = raw.get(10..).unwrap_or("");
    (!time.ends_with('Z') && !time.contains('+') && !time.contains('-'))
        .then_some(raw)
        .and_then(date_parse_ms)
}

/// The local offset of a trajectory's naive timestamps, from the file mtime
/// against the newest naive timestamp in the file (`end_time` when the run
/// finished, else the last interaction or agent step). `Some(0)` when the file
/// has no naive timestamp to shift; `None` when the gap is more than 14 hours or
/// further than `MAX_RESIDUAL_MS` from the 15-minute grid.
fn naive_offset_ms(parsed: &Value, interactions: &[Value], file: &str) -> Option<i64> {
    let steps = parsed.get("agent_steps").and_then(Value::as_array);
    let anchor = interactions
        .iter()
        .chain(steps.into_iter().flatten())
        .filter_map(|record| record.get("timestamp"))
        .chain(parsed.get("start_time"))
        .chain(parsed.get("end_time"))
        .filter_map(naive_ms)
        .max();
    let Some(anchor) = anchor else {
        return Some(0);
    };
    let mtime = fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    let gap = mtime.checked_sub(anchor)?;
    let rounded = (gap as f64 / QUARTER_HOUR_MS as f64).round() as i64 * QUARTER_HOUR_MS;
    (rounded.abs() <= MAX_OFFSET_MS && (gap - rounded).abs() < MAX_RESIDUAL_MS).then_some(rounded)
}

/// List the trajectory files directly under `dir` (non-recursive) whose names
/// match the documented `trajectory_<YYYYMMDD_HHMMSS>.json` shape, returned
/// sorted. The evidenced contract places trajectories only at
/// `<cwd>/trajectories/trajectory_*.json`, so discovery does not recurse into
/// subdirectories and does not accept other `*.json` names.
fn list_trajectory_files(dir: &Path) -> Vec<String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(is_trajectory_file_name)
                .unwrap_or(false)
        })
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// True when `name` matches `trajectory_*.json`.
fn is_trajectory_file_name(name: &str) -> bool {
    name.starts_with("trajectory_") && name.ends_with(".json")
}

/// Read all trae-agent usage events reachable from `cwd`. Each
/// `<cwd>/trajectories/trajectory_*.json` is read once from its
/// `llm_interactions`.
pub fn read_trae(cwd: &Path) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();

    let trajectories = trae_trajectories_root(cwd);
    if !dir_exists(&trajectories) {
        return ReaderResult::default();
    }

    let files = list_trajectory_files(&trajectories);
    for file in &files {
        events.extend(read_trajectory(file, &mut skipped, &mut warnings));
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
    use std::fs;

    fn write_trajectory(cwd: &Path, name: &str, body: &str) {
        let dir = cwd.join("trajectories");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), body).unwrap();
    }

    const NAIVE_RUN: &str = r#"{"task":"t","start_time":"2026-11-01T12:59:56.291241","end_time":"2026-11-01T13:00:35.669971","model":"qwen","llm_interactions":[{"timestamp":"2026-11-01T13:00:04.885339","response":{"model":"qwen","usage":{"input_tokens":10,"output_tokens":5}}},{"timestamp":"2026-11-01T13:00:35.650334+00:00","response":{"model":"qwen","usage":{"input_tokens":10,"output_tokens":5}}}]}"#;

    /// Set the file mtime to `end_time` (as UTC) plus `delta_ms`.
    fn set_mtime(cwd: &Path, name: &str, delta_ms: i64) {
        let at = std::time::UNIX_EPOCH
            + std::time::Duration::from_millis(
                (date_parse_ms("2026-11-01T13:00:35.669971").unwrap() + delta_ms) as u64,
            );
        fs::File::options()
            .write(true)
            .open(cwd.join("trajectories").join(name))
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    #[test]
    fn naive_timestamps_are_shifted_by_the_mtime_anchor() {
        let dir = scratch_dir("anchor");
        write_trajectory(&dir, "trajectory_run1.json", NAIVE_RUN);
        set_mtime(&dir, "trajectory_run1.json", 8 * 3_600_000 + 2_300);
        let r = read_trae(&dir);
        assert!(r.warnings.is_empty());
        assert_eq!(r.events[0].timestamp, "2026-11-01T21:00:04.885Z");
        assert_eq!(
            r.events[1].timestamp, "2026-11-01T13:00:35.650Z",
            "an offset-aware stamp is left alone"
        );
    }

    #[test]
    fn negative_offsets_round_to_the_quarter_hour() {
        let dir = scratch_dir("anchor-neg");
        write_trajectory(&dir, "trajectory_run1.json", NAIVE_RUN);
        set_mtime(
            &dir,
            "trajectory_run1.json",
            -(5 * 3_600_000 + 30 * 60_000) + 40_000,
        );
        let r = read_trae(&dir);
        assert!(r.warnings.is_empty());
        assert_eq!(r.events[0].timestamp, "2026-11-01T07:30:04.885Z");
    }

    #[test]
    fn stale_mtime_books_naive_as_utc_with_one_warning() {
        let dir = scratch_dir("anchor-stale");
        write_trajectory(&dir, "trajectory_run1.json", NAIVE_RUN);
        set_mtime(&dir, "trajectory_run1.json", 20 * 3_600_000);
        let r = read_trae(&dir);
        assert_eq!(r.events[0].timestamp, "2026-11-01T13:00:04.885Z");
        assert_eq!(r.warnings.len(), 1);
        assert!(
            r.warnings[0]
                .message
                .starts_with("naive timestamps, no usable anchor in ")
        );
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn a_copy_off_the_grid_by_minutes_has_no_anchor() {
        let dir = scratch_dir("anchor-copy");
        write_trajectory(&dir, "trajectory_run1.json", NAIVE_RUN);
        set_mtime(&dir, "trajectory_run1.json", 8 * 3_600_000 + 5 * 60_000);
        let r = read_trae(&dir);
        assert_eq!(r.warnings.len(), 1);
        assert_eq!(r.events[0].timestamp, "2026-11-01T13:00:04.885Z");
    }

    #[test]
    fn the_last_agent_step_anchors_a_run_without_end_time() {
        let dir = scratch_dir("anchor-step");
        let body = NAIVE_RUN.replace(
            r#""end_time":"2026-11-01T13:00:35.669971","#,
            r#""agent_steps":[{"timestamp":"2026-11-01T13:04:00.000000"}],"#,
        );
        write_trajectory(&dir, "trajectory_run1.json", &body);
        set_mtime(&dir, "trajectory_run1.json", 3_600_000 + 204_830);
        let r = read_trae(&dir);
        assert!(r.warnings.is_empty());
        assert_eq!(r.events[0].timestamp, "2026-11-01T14:00:04.885Z");
    }

    #[test]
    fn a_numeric_string_stamp_survives_an_anchored_file() {
        let dir = scratch_dir("anchor-numeric");
        let body = NAIVE_RUN.replace(
            r#""timestamp":"2026-11-01T13:00:04.885339""#,
            r#""timestamp":"1793534404885""#,
        );
        write_trajectory(&dir, "trajectory_run1.json", &body);
        set_mtime(&dir, "trajectory_run1.json", 8 * 3_600_000 + 2_300);
        let r = read_trae(&dir);
        assert!(r.warnings.is_empty());
        assert_eq!(r.events[0].timestamp, "2026-11-01T12:00:04.885Z");
    }

    #[test]
    fn newest_timestamp_anchors_a_run_without_end_time() {
        let dir = scratch_dir("anchor-newest");
        let body = NAIVE_RUN.replace(r#""end_time":"2026-11-01T13:00:35.669971","#, "");
        write_trajectory(&dir, "trajectory_run1.json", &body);
        set_mtime(&dir, "trajectory_run1.json", 3_600_000 - 30_000);
        let r = read_trae(&dir);
        assert!(r.warnings.is_empty());
        assert_eq!(r.events[0].timestamp, "2026-11-01T14:00:04.885Z");
    }

    #[test]
    fn trajectories_root_resolves_under_cwd() {
        let cwd = PathBuf::from("/work/proj");
        assert_eq!(trae_trajectories_root(&cwd), cwd.join("trajectories"));
    }

    #[test]
    fn no_cwd_yields_empty() {
        let ctx = ReaderContext::new("/home/u");
        assert!(TraeReader.read(&ctx).events.is_empty());
    }

    #[test]
    fn reads_llm_interactions_with_cache_splits_and_reasoning() {
        let dir = scratch_dir("basic");
        write_trajectory(
            &dir,
            "trajectory_20260801_100000.json",
            r#"{"task":"t","start_time":"2026-08-01T10:00:00","model":"traj-model","llm_interactions":[{"timestamp":"2026-08-01T10:00:05","provider":"anthropic","model":"claude","response":{"model":"claude","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":200,"cache_read_input_tokens":300,"reasoning_tokens":40}}}],"agent_steps":[{"step_number":1,"llm_response":{"model":"claude","usage":{"input_tokens":9999,"output_tokens":9999}}}]}"#,
        );
        let events = read_trae(&dir).events;
        assert_eq!(
            events.len(),
            1,
            "one event per interaction; agent_steps ignored"
        );
        assert_eq!(events[0].model, "claude");
        assert_eq!(events[0].tokens.input, 100);
        assert_eq!(events[0].tokens.output, 50);
        assert_eq!(events[0].tokens.cache_write, 200);
        assert_eq!(events[0].tokens.cache_read, 300);
        assert_eq!(events[0].tokens.reasoning, 40);
        assert_eq!(events[0].timestamp, "2026-08-01T10:00:05.000Z");
        assert_eq!(events[0].session_id, "trajectory_20260801_100000");
    }

    #[test]
    fn null_cache_fields_are_treated_as_zero() {
        let dir = scratch_dir("nullcache");
        write_trajectory(
            &dir,
            "trajectory_20260801_100000.json",
            r#"{"model":"m","llm_interactions":[{"timestamp":"2026-08-01T10:00:00","response":{"usage":{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":null,"cache_read_input_tokens":null,"reasoning_tokens":null}}}]}"#,
        );
        let events = read_trae(&dir).events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tokens.cache_read, 0);
        assert_eq!(events[0].tokens.cache_write, 0);
        assert_eq!(events[0].tokens.reasoning, 0);
        assert_eq!(events[0].model, "m", "falls back to trajectory model");
    }

    #[test]
    fn zero_usage_interactions_are_dropped() {
        let dir = scratch_dir("zero");
        write_trajectory(
            &dir,
            "trajectory_20260801_100000.json",
            r#"{"model":"m","llm_interactions":[{"response":{"usage":{"input_tokens":0,"output_tokens":0}}}]}"#,
        );
        assert!(read_trae(&dir).events.is_empty());
    }

    #[test]
    fn missing_trajectories_dir_yields_empty() {
        let dir = scratch_dir("absent");
        assert!(read_trae(&dir).events.is_empty());
    }

    #[test]
    fn malformed_interactions_are_skipped_with_diagnostics() {
        let dir = scratch_dir("malformed-interaction");
        write_trajectory(
            &dir,
            "trajectory_20260801_100000.json",
            r#"{"model":"m","llm_interactions":[{"response":{"usage":{"input_tokens":"oops","output_tokens":5}}},{"response":{"usage":{"input_tokens":10,"output_tokens":5}}}]}"#,
        );
        let file = dir
            .join("trajectories")
            .join("trajectory_20260801_100000.json")
            .to_string_lossy()
            .into_owned();
        let result = read_trae(&dir);
        assert_eq!(result.events.len(), 1, "valid interaction still emitted");
        assert_eq!(result.skipped, vec![format!("{file}:1")]);
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed trae interaction {file}:1\n")
        );
    }

    #[test]
    fn absent_or_null_required_cells_skip_the_interaction() {
        let dir = scratch_dir("required-cells");
        write_trajectory(
            &dir,
            "trajectory_20260803_100000.json",
            r#"{"model":"m","llm_interactions":[{"response":{"usage":{"output_tokens":5}}},{"response":{"usage":{"input_tokens":10,"output_tokens":null}}},{"response":{"usage":{"cache_read_input_tokens":7,"reasoning_tokens":3}}},{"response":{"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":null,"cache_creation_input_tokens":null,"reasoning_tokens":null}}}]}"#,
        );
        let file = dir
            .join("trajectories")
            .join("trajectory_20260803_100000.json")
            .to_string_lossy()
            .into_owned();
        let result = read_trae(&dir);
        assert_eq!(
            result.events.len(),
            1,
            "only the fully valid interaction emits"
        );
        assert_eq!(result.events[0].tokens.input, 10);
        assert_eq!(result.events[0].tokens.cache_read, 0);
        assert_eq!(
            result.skipped,
            vec![
                format!("{file}:1"),
                format!("{file}:2"),
                format!("{file}:3")
            ]
        );
        assert_eq!(
            result.warnings[0].message,
            format!("skipping malformed trae interaction {file}:1\n")
        );
    }

    #[test]
    fn invalid_trajectory_files_get_file_level_diagnostics() {
        let dir = scratch_dir("invalid-file");
        // non-array llm_interactions
        write_trajectory(
            &dir,
            "trajectory_20260801_100000.json",
            r#"{"model":"m","llm_interactions":{"nope":true}}"#,
        );
        // non-object root
        write_trajectory(&dir, "trajectory_20260802_100000.json", r#"[1,2,3]"#);
        let result = read_trae(&dir);
        assert!(result.events.is_empty());
        assert_eq!(result.skipped.len(), 2, "both invalid files skipped");
        assert!(
            result
                .warnings
                .iter()
                .all(|w| w.message.starts_with("skipping malformed trae trajectory ")),
            "file-level malformed diagnostics for both"
        );
    }

    #[test]
    fn discovery_excludes_nested_and_unmatched_json() {
        let dir = scratch_dir("discovery");
        let trajectories = dir.join("trajectories");
        fs::create_dir_all(&trajectories).unwrap();
        // a valid, matched trajectory directly under trajectories/
        fs::write(
            trajectories.join("trajectory_20260801_100000.json"),
            r#"{"model":"m","llm_interactions":[{"response":{"usage":{"input_tokens":10,"output_tokens":5}}}]}"#,
        )
        .unwrap();
        // an unmatched name directly under trajectories/ (must be ignored)
        fs::write(
            trajectories.join("other.json"),
            r#"{"model":"m","llm_interactions":[{"response":{"usage":{"input_tokens":99,"output_tokens":99}}}]}"#,
        )
        .unwrap();
        // a nested trajectory-named file (must be ignored: non-recursive)
        let nested = trajectories.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("trajectory_20260803_100000.json"),
            r#"{"model":"m","llm_interactions":[{"response":{"usage":{"input_tokens":77,"output_tokens":77}}}]}"#,
        )
        .unwrap();
        let result = read_trae(&dir);
        assert_eq!(
            result.events.len(),
            1,
            "only the matched top-level file reads"
        );
        assert_eq!(result.events[0].tokens.input, 10);
        assert!(result.skipped.is_empty());
    }
}

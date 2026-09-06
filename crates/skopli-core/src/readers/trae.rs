use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, dir_exists, file_mtime_iso, file_stem_name,
    finite_number, is_record, parse_timestamp_to_iso, read_json,
};
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

/// An optional token cell. The recorder writes `cache_creation_input_tokens`,
/// `cache_read_input_tokens`, and `reasoning_tokens` as JSON `null` when the
/// provider reports no usage, so absent or `null` is `0`; a populated cell
/// that is not a non-negative finite number is invalid (`None`), which marks
/// the whole interaction malformed. Mirrors the null-vs-populated-invalid
/// split used by the cherrystudio reader.
fn optional_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
}

/// A required token cell. The recorder always writes `input_tokens` and
/// `output_tokens` as numbers (zero when the provider reports no usage), so an
/// absent, `null`, or invalid cell marks the interaction malformed.
fn required_cell(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => finite_number(v).filter(|n| *n >= 0.0).map(|n| n as u64),
    }
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
    let fallback_timestamp = parsed
        .get("start_time")
        .and_then(parse_timestamp_to_iso)
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
            .and_then(parse_timestamp_to_iso)
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
    use std::fs;

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-trae-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn write_trajectory(cwd: &Path, name: &str, body: &str) {
        let dir = cwd.join("trajectories");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), body).unwrap();
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
        let dir = tmp_dir("basic");
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
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn null_cache_fields_are_treated_as_zero() {
        let dir = tmp_dir("nullcache");
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
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zero_usage_interactions_are_dropped() {
        let dir = tmp_dir("zero");
        write_trajectory(
            &dir,
            "trajectory_20260801_100000.json",
            r#"{"model":"m","llm_interactions":[{"response":{"usage":{"input_tokens":0,"output_tokens":0}}}]}"#,
        );
        assert!(read_trae(&dir).events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_trajectories_dir_yields_empty() {
        let dir = tmp_dir("absent");
        assert!(read_trae(&dir).events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_interactions_are_skipped_with_diagnostics() {
        let dir = tmp_dir("malformed-interaction");
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
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absent_or_null_required_cells_skip_the_interaction() {
        let dir = tmp_dir("required-cells");
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
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_trajectory_files_get_file_level_diagnostics() {
        let dir = tmp_dir("invalid-file");
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
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discovery_excludes_nested_and_unmatched_json() {
        let dir = tmp_dir("discovery");
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
        fs::remove_dir_all(&dir).ok();
    }
}

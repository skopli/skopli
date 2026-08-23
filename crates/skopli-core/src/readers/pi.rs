use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    JsonlLine, ReaderResult, ReaderWarning, as_string, file_mtime_iso, finite_number, is_record,
    list_files, read_jsonl_lines,
};
use crate::types::{TokenCounts, UsageEvent};

/// The pi reader wired into the harness registry.
pub struct PiReader;

impl Reader for PiReader {
    fn harness_id(&self) -> &'static str {
        "pi"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = pi_sessions_root(ctx.env("PI_AGENT_DIR"), ctx.home());
        read_pi_root(&root, "pi")
    }
}

/// The omp reader wired into the harness registry.
pub struct OmpReader;

impl Reader for OmpReader {
    fn harness_id(&self) -> &'static str {
        "omp"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let root = omp_sessions_root(ctx.home());
        read_pi_root(&root, "omp")
    }
}

/// The Prime Agent reader wired into the harness registry. Prime Agent is a
/// pi-ai family harness, so it shares the pi JSONL logic; only the sessions
/// roots differ (`~/.prime/agent/sessions`, env overrides
/// `PRIME_AGENT_SESSION_DIR` and `PRIME_AGENT_CODING_AGENT_DIR`).
pub struct PrimeReader;

impl Reader for PrimeReader {
    fn harness_id(&self) -> &'static str {
        "prime"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = prime_sessions_roots(
            ctx.env("PRIME_AGENT_SESSION_DIR"),
            ctx.env("PRIME_AGENT_CODING_AGENT_DIR"),
            ctx.home(),
        );
        read_pi_roots(&roots, "prime")
    }
}

/// The gajae-code (gjc) reader wired into the harness registry. Another pi-ai
/// family harness sharing the pi JSONL logic; roots are `~/.gjc/agent/sessions`
/// (env overrides `GJC_CONFIG_DIR` / `GJC_CODING_AGENT_DIR`) plus the XDG-aware
/// `$XDG_STATE_HOME/gjc/agent/sessions`.
pub struct GajaeReader;

impl Reader for GajaeReader {
    fn harness_id(&self) -> &'static str {
        "gajae"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = gajae_sessions_roots(
            ctx.env("GJC_CODING_AGENT_DIR"),
            ctx.env("GJC_CONFIG_DIR"),
            ctx.env("XDG_STATE_HOME"),
            ctx.home(),
        );
        read_pi_roots(&roots, "gajae")
    }
}

/// The Kimchi Coding reader wired into the harness registry. Kimchi is built on
/// `pi-coding-agent`, so it shares the pi JSONL logic; roots are the config-dir
/// candidates (`~/.config/kimchi/agent/sessions`, env `KIMCHI_CONFIG_PATH`) and
/// the pi agent default (`~/.pi/agent/sessions`).
pub struct KimchiReader;

impl Reader for KimchiReader {
    fn harness_id(&self) -> &'static str {
        "kimchi"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = kimchi_sessions_roots(ctx.env("KIMCHI_CONFIG_PATH"), ctx.home());
        read_pi_roots(&roots, "kimchi")
    }
}

/// Resolve the pi sessions root. Faithful port of `piSessionsRoot`.
pub fn pi_sessions_root(override_dir: Option<&str>, home: &Path) -> PathBuf {
    if let Some(v) = override_dir
        && !v.is_empty()
    {
        return PathBuf::from(v).join("sessions");
    }
    home.join(".pi").join("agent").join("sessions")
}

/// Resolve the omp sessions root. Faithful port of `ompSessionsRoot`.
pub fn omp_sessions_root(home: &Path) -> PathBuf {
    home.join(".omp").join("agent").join("sessions")
}

/// Resolve the Prime Agent sessions roots. `PRIME_AGENT_SESSION_DIR` is a
/// sessions dir used verbatim; `PRIME_AGENT_CODING_AGENT_DIR` is an agent dir
/// with `sessions` appended; otherwise the default `~/.prime/agent/sessions`.
pub fn prime_sessions_roots(
    session_dir: Option<&str>,
    agent_dir: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(v) = session_dir
        && !v.is_empty()
    {
        return vec![PathBuf::from(v)];
    }
    if let Some(v) = agent_dir
        && !v.is_empty()
    {
        return vec![PathBuf::from(v).join("sessions")];
    }
    vec![home.join(".prime").join("agent").join("sessions")]
}

/// Resolve the gajae-code (gjc) sessions roots. `GJC_CODING_AGENT_DIR` is an
/// agent dir with `sessions` appended; `GJC_CONFIG_DIR` is the `.gjc` config
/// root with `agent/sessions` appended; otherwise the default
/// `~/.gjc/agent/sessions` plus the XDG-aware `$XDG_STATE_HOME/gjc/agent/sessions`.
pub fn gajae_sessions_roots(
    agent_dir: Option<&str>,
    config_dir: Option<&str>,
    xdg_state_home: Option<&str>,
    home: &Path,
) -> Vec<PathBuf> {
    if let Some(v) = agent_dir.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v).join("sessions")];
    }
    if let Some(v) = config_dir.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v).join("agent").join("sessions")];
    }
    let mut roots = vec![home.join(".gjc").join("agent").join("sessions")];
    if let Some(v) = xdg_state_home
        && !v.is_empty()
    {
        roots.push(PathBuf::from(v).join("gjc").join("agent").join("sessions"));
    }
    roots
}

/// Resolve the Kimchi Coding sessions roots. A config-dir override
/// (`KIMCHI_CONFIG_PATH`) yields `<config>/agent/sessions`; otherwise the
/// documented config-dir default (`~/.config/kimchi/agent/sessions`) and the pi
/// agent default (`~/.pi/agent/sessions`).
pub fn kimchi_sessions_roots(config_path: Option<&str>, home: &Path) -> Vec<PathBuf> {
    if let Some(v) = config_path
        && !v.is_empty()
    {
        return vec![PathBuf::from(v).join("agent").join("sessions")];
    }
    vec![
        home.join(".config")
            .join("kimchi")
            .join("agent")
            .join("sessions"),
        home.join(".pi").join("agent").join("sessions"),
    ]
}

/// Read pi-family usage events from several sessions roots into one result,
/// deduplicating by message id across every root.
pub fn read_pi_roots(roots: &[PathBuf], harness: &str) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for root in roots {
        read_pi_root_into(
            root,
            harness,
            &mut events,
            &mut skipped,
            &mut warnings,
            &mut seen,
        );
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

/// Read pi/omp usage events from a sessions root. Faithful port of `readPiRoot`.
pub fn read_pi_root(root: &Path, harness: &str) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    read_pi_root_into(
        root,
        harness,
        &mut events,
        &mut skipped,
        &mut warnings,
        &mut seen,
    );
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

/// Read pi-family usage events from `root` into the shared accumulators,
/// deduplicating by message id via `seen`. Faithful port of `readPiRoot`'s body.
fn read_pi_root_into(
    root: &Path,
    harness: &str,
    events: &mut Vec<UsageEvent>,
    skipped: &mut Vec<String>,
    warnings: &mut Vec<ReaderWarning>,
    seen: &mut HashSet<String>,
) {
    for file in list_files(root, |name| name.ends_with(".jsonl")) {
        let lines = match read_jsonl_lines(&file, skipped, warnings) {
            Some(l) => l,
            None => continue,
        };
        let mut session_id: Option<String> = None;
        let mut subagent = false;
        let mut previous_was_user = false;
        for JsonlLine { index, value } in &lines {
            if !is_record(value) {
                continue;
            }
            let line_type = value.get("type").and_then(Value::as_str);
            if line_type == Some("session") {
                session_id = value.get("id").and_then(as_string);
                subagent = value.get("rlmDepth").and_then(finite_number).unwrap_or(0.0) > 0.0;
                continue;
            }
            if line_type != Some("message") {
                continue;
            }
            let message = match value.get("message") {
                Some(m) if is_record(m) => m,
                _ => continue,
            };
            let role = message.get("role").and_then(Value::as_str);
            if role == Some("user") {
                previous_was_user = true;
                continue;
            }
            if role != Some("assistant") {
                continue;
            }
            let usage = message.get("usage");
            let model = message.get("model").and_then(as_string);
            let id = value.get("id").and_then(as_string);
            let usage_rec = usage.filter(|u| is_record(u));
            let input = usage_rec
                .and_then(|u| u.get("input"))
                .and_then(finite_number);
            let output = usage_rec
                .and_then(|u| u.get("output"))
                .and_then(finite_number);
            let cache_read = usage_rec
                .and_then(|u| u.get("cacheRead"))
                .and_then(finite_number);
            let cache_write = usage_rec
                .and_then(|u| u.get("cacheWrite"))
                .and_then(finite_number);

            let (model, id, input, output, cache_read, cache_write) =
                match (model, id, input, output, cache_read, cache_write) {
                    (Some(m), Some(i), Some(inp), Some(out), Some(cr), Some(cw)) => {
                        (m, i, inp, out, cr, cw)
                    }
                    _ => {
                        let location = format!("{file}:{}", index + 1);
                        skipped.push(location.clone());
                        warnings.push(ReaderWarning {
                            message: format!("skipping malformed {harness} usage {location}\n"),
                        });
                        previous_was_user = false;
                        continue;
                    }
                };
            let turn = previous_was_user;
            let event = UsageEvent {
                harness: harness.to_owned(),
                timestamp: value
                    .get("timestamp")
                    .and_then(as_string)
                    .unwrap_or_else(|| file_mtime_iso(&file)),
                session_id: session_id.clone().unwrap_or_else(|| file.clone()),
                message_id: id,
                turn,
                subagent,
                model,
                tokens: TokenCounts {
                    input: input.max(0.0) as u64,
                    output: output.max(0.0) as u64,
                    cache_read: cache_read.max(0.0) as u64,
                    cache_write: cache_write.max(0.0) as u64,
                    cache_write1h: None,
                    reasoning: 0,
                },
                calls: None,
                cost_usd: None,
                workspace: None,
                title: None,
            };
            let tokens_json = serde_json::to_string(&event.tokens).unwrap_or_default();
            let dedup_key = format!(
                "{}:{}:{}:{}",
                event.session_id, event.message_id, event.model, tokens_json
            );
            previous_was_user = false;
            if seen.contains(&dedup_key) {
                continue;
            }
            seen.insert(dedup_key);
            events.push(event);
        }
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
    fn gajae_coding_agent_dir_appends_sessions() {
        let roots = gajae_sessions_roots(Some("/custom/agent"), None, None, &home());
        assert_eq!(roots, vec![PathBuf::from("/custom/agent").join("sessions")]);
    }

    #[test]
    fn gajae_config_dir_appends_agent_sessions() {
        let roots = gajae_sessions_roots(None, Some("/custom/gjc"), None, &home());
        assert_eq!(
            roots,
            vec![PathBuf::from("/custom/gjc").join("agent").join("sessions")]
        );
    }

    #[test]
    fn gajae_coding_agent_dir_wins_over_config_dir() {
        let roots = gajae_sessions_roots(Some("/agent"), Some("/gjc"), None, &home());
        assert_eq!(roots, vec![PathBuf::from("/agent").join("sessions")]);
    }

    #[test]
    fn gajae_default_home_and_xdg_state() {
        let roots = gajae_sessions_roots(None, None, Some("/xdg/state"), &home());
        assert_eq!(
            roots,
            vec![
                home().join(".gjc").join("agent").join("sessions"),
                PathBuf::from("/xdg/state")
                    .join("gjc")
                    .join("agent")
                    .join("sessions"),
            ]
        );
    }

    #[test]
    fn gajae_default_home_only_without_xdg() {
        let roots = gajae_sessions_roots(None, None, None, &home());
        assert_eq!(
            roots,
            vec![home().join(".gjc").join("agent").join("sessions")]
        );
    }

    #[test]
    fn gajae_empty_overrides_fall_through_to_default() {
        let roots = gajae_sessions_roots(Some(""), Some(""), Some(""), &home());
        assert_eq!(
            roots,
            vec![home().join(".gjc").join("agent").join("sessions")]
        );
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-pi-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn write_session(dir: &Path, file: &str, session_id: &str, message_id: &str) {
        fs::create_dir_all(dir).unwrap();
        let lines = [
            format!("{{\"type\":\"session\",\"id\":\"{session_id}\"}}"),
            "{\"type\":\"message\",\"message\":{\"role\":\"user\"}}".to_owned(),
            format!(
                "{{\"type\":\"message\",\"id\":\"{message_id}\",\"timestamp\":\"2026-08-01T10:00:00.000Z\",\"message\":{{\"role\":\"assistant\",\"model\":\"gpt-5\",\"usage\":{{\"input\":8,\"output\":3,\"cacheRead\":5,\"cacheWrite\":7}}}}}}"
            ),
        ];
        fs::write(dir.join(file), lines.join("\n")).unwrap();
    }

    #[test]
    fn dedup_mirrored_session_across_roots_and_keeps_distinct_session() {
        let base = tmp_dir("dedup");
        let home_root = base.join("home");
        let xdg_root = base.join("xdg");
        let home_sessions = home_root.join(".gjc").join("agent").join("sessions");
        let xdg_sessions = xdg_root.join("gjc").join("agent").join("sessions");
        // Same session (same session id + message id + content) mirrored under
        // both default candidate roots must be counted once.
        write_session(&home_sessions, "s1.jsonl", "session-1", "msg-1");
        write_session(&xdg_sessions, "s1.jsonl", "session-1", "msg-1");
        // A distinct session that reuses the same message id must survive.
        write_session(&home_sessions, "s2.jsonl", "session-2", "msg-1");

        let ctx = ReaderContext::new(&home_root)
            .with_env("XDG_STATE_HOME", xdg_root.to_string_lossy().to_string());
        let result = GajaeReader.read(&ctx);

        assert_eq!(result.events.len(), 2, "one mirrored + one distinct event");
        let mut sessions: Vec<&str> = result
            .events
            .iter()
            .map(|e| e.session_id.as_str())
            .collect();
        sessions.sort_unstable();
        assert_eq!(sessions, vec!["session-1", "session-2"]);

        fs::remove_dir_all(&base).ok();
    }
}

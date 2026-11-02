use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, dir_exists, file_stem_name, is_record, list_files,
    normalize_workspace, optional_cell, parse_timestamp_to_iso, read_jsonl_lines, required_cell,
};
use crate::types::{TokenCounts, UsageEvent};

/// The Command Code reader wired into the harness registry. Command Code keeps
/// one transcript per session at `~/.commandcode/projects/<cwd-slug>/<session
/// uuid>.jsonl`: a `session` header line followed by `message` lines, where the
/// assistant line of each turn carries a top-level `usage` object and `model`.
/// The sidecars beside it (`.meta.json`, `.share.json`, `.checkpoints.jsonl`,
/// `.prompts.jsonl`, `.scheduled-tasks.json`, `.v2.bak`) hold no usage and are
/// told apart by the harness's own transcript test, mirrored in
/// `is_transcript_name`.
pub struct CommandcodeReader;

impl Reader for CommandcodeReader {
    fn harness_id(&self) -> &'static str {
        "commandcode"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        read_commandcode(&commandcode_projects(ctx.home()))
    }
}

/// The projects store, `<home>/.commandcode/projects`. Command Code derives the
/// root from `HOME` (`USERPROFILE` on Windows) alone, with no override variable.
pub fn commandcode_projects(home: &Path) -> PathBuf {
    home.join(".commandcode").join("projects")
}

const UNKNOWN_MODEL: &str = "commandcode-unknown";

/// Command Code's `isSessionTranscriptFileName`, verbatim.
fn is_transcript_name(name: &str) -> bool {
    name.ends_with(".jsonl")
        && !name.contains(".checkpoints.")
        && !name.contains(".prompts.")
        && !name.contains(".v2.bak")
}

/// `inputTokens` is the full prompt, cached prefix included (it equals the
/// wire's `prompt_tokens`), so the cache read is subtracted to count each token
/// once. A cell that is present but not a number is malformed (`None`).
fn parse_usage(usage: &Value) -> Option<TokenCounts> {
    let prompt = required_cell(usage.get("inputTokens"))?;
    let output = required_cell(usage.get("outputTokens"))?;
    let cache_read = optional_cell(usage.get("cacheReadTokens"))?;
    let cache_write = optional_cell(usage.get("cacheWriteTokens"))?;
    Some(TokenCounts {
        input: prompt.saturating_sub(cache_read),
        output,
        cache_read,
        cache_write,
        cache_write1h: None,
        reasoning: 0,
    })
}

/// Read every session transcript under the projects store. One event per line
/// with a `usage` object; a repeated message id is a rewrite and the last line
/// wins.
pub fn read_commandcode(projects: &Path) -> ReaderResult {
    let mut result = ReaderResult::default();
    if !dir_exists(projects) {
        return result;
    }
    let files = list_files(projects, is_transcript_name);
    for file in &files {
        let Some(lines) = read_jsonl_lines(file, &mut result.skipped, &mut result.warnings) else {
            continue;
        };
        let session_id = file_stem_name(file);
        let header = lines
            .iter()
            .map(|l| &l.value)
            .find(|v| v.get("type").and_then(Value::as_str) == Some("session"));
        let workspace = header
            .and_then(|h| h.get("cwd"))
            .and_then(normalize_workspace);
        let mut by_id: HashMap<String, usize> = HashMap::new();
        let mut events: Vec<UsageEvent> = Vec::new();
        for line in &lines {
            let Some(usage) = line.value.get("usage").filter(|u| is_record(u)) else {
                continue;
            };
            let loc = format!("{file}:{}", line.index + 1);
            let Some(tokens) = parse_usage(usage) else {
                result.warnings.push(ReaderWarning {
                    message: format!("skipping malformed usage at {loc}\n"),
                });
                result.skipped.push(loc);
                continue;
            };
            let Some(timestamp) = line.value.get("timestamp").and_then(parse_timestamp_to_iso)
            else {
                result.warnings.push(ReaderWarning {
                    message: format!("skipping message without a timestamp at {loc}\n"),
                });
                result.skipped.push(loc);
                continue;
            };
            let message_id = line
                .value
                .get("id")
                .and_then(as_string)
                .filter(|id| !id.trim().is_empty())
                .unwrap_or(loc);
            let event = UsageEvent {
                harness: "commandcode".to_owned(),
                timestamp,
                session_id: session_id.clone(),
                message_id: message_id.clone(),
                turn: true,
                subagent: false,
                model: line
                    .value
                    .get("model")
                    .and_then(as_string)
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| UNKNOWN_MODEL.to_owned()),
                tokens,
                calls: None,
                cost_usd: None,
                workspace: workspace.clone(),
                title: None,
            };
            match by_id.get(&message_id) {
                Some(&i) => events[i] = event,
                None => {
                    by_id.insert(message_id, events.len());
                    events.push(event);
                }
            }
        }
        result.events.append(&mut events);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::readers::shared::{scratch_dir, test_home};

    const HEADER: &str = r#"{"type":"session","version":3,"id":"s1","timestamp":"2026-10-25T10:21:20.528Z","cwd":"/work/p-limit"}"#;
    const USER: &str = r#"{"type":"message","id":"u1","parentId":null,"timestamp":"2026-10-25T10:24:58.100Z","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#;
    const ASSISTANT: &str = r#"{"type":"message","id":"a1","parentId":"u1","timestamp":"2026-10-25T10:24:58.100Z","message":{"role":"assistant","content":[]},"usage":{"inputTokens":14607,"outputTokens":40,"cacheReadTokens":6955,"cacheWriteTokens":0},"model":"llama/qwen"}"#;
    const ASSISTANT2: &str = r#"{"type":"message","id":"a2","parentId":"u2","timestamp":"2026-10-25T10:27:34.501Z","message":{"role":"assistant","content":[]},"usage":{"inputTokens":14670,"outputTokens":42,"cacheReadTokens":14646,"cacheWriteTokens":0},"model":"llama/qwen"}"#;

    fn write_session(lines: &[&str]) -> (PathBuf, PathBuf) {
        let home = scratch_dir("commandcode");
        let dir = commandcode_projects(&home).join("work-p-limit");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("0f1e2d3c-0000-4000-8000-000000000001.jsonl");
        fs::write(&file, lines.join("\n") + "\n").unwrap();
        for (sidecar, body) in [
            ("checkpoints.jsonl", "{\"turnNumber\":1}\n"),
            ("prompts.jsonl", "not a transcript line\n"),
            ("meta.json", "{\"entrypoint\":\"print\"}"),
            ("v2.bak", "{"),
        ] {
            fs::write(
                dir.join(format!("0f1e2d3c-0000-4000-8000-000000000001.{sidecar}")),
                body,
            )
            .unwrap();
        }
        (home, file)
    }

    #[test]
    fn projects_root_is_dot_commandcode_under_home() {
        assert_eq!(
            commandcode_projects(&test_home()),
            test_home().join(".commandcode").join("projects")
        );
    }

    #[test]
    fn missing_store_yields_nothing() {
        let r = read_commandcode(&scratch_dir("commandcode-empty").join("absent"));
        assert!(r.events.is_empty() && r.warnings.is_empty());
    }

    #[test]
    fn session_and_user_lines_carry_no_usage() {
        let (home, _) = write_session(&[HEADER, USER]);
        let r = CommandcodeReader.read(&ReaderContext::new(&home));
        assert!(r.events.is_empty());
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn one_event_per_assistant_line_with_uncached_input() {
        let (home, _) = write_session(&[HEADER, USER, ASSISTANT, ASSISTANT2]);
        let r = CommandcodeReader.read(&ReaderContext::new(&home));
        assert_eq!(r.events.len(), 2);
        let e = &r.events[0];
        assert_eq!(e.session_id, "0f1e2d3c-0000-4000-8000-000000000001");
        assert_eq!(e.message_id, "a1");
        assert_eq!(e.timestamp, "2026-10-25T10:24:58.100Z");
        assert_eq!(e.model, "llama/qwen");
        assert_eq!(e.tokens.input, 14607 - 6955);
        assert_eq!(e.tokens.cache_read, 6955);
        assert_eq!(e.tokens.output, 40);
        assert_eq!(e.workspace.as_deref(), Some("/work/p-limit"));
        assert_eq!(r.events[1].tokens.input, 14670 - 14646);
    }

    #[test]
    fn repeated_message_id_is_a_rewrite() {
        let rewrite = ASSISTANT.replace("\"outputTokens\":40", "\"outputTokens\":41");
        let (home, _) = write_session(&[HEADER, ASSISTANT, &rewrite]);
        let r = CommandcodeReader.read(&ReaderContext::new(&home));
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.events[0].tokens.output, 41);
    }

    #[test]
    fn malformed_line_is_skipped_with_a_warning() {
        let (home, file) = write_session(&[HEADER, "{not json", ASSISTANT]);
        let r = CommandcodeReader.read(&ReaderContext::new(&home));
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.skipped, vec![format!("{}:2", file.display())]);
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn non_numeric_usage_cell_is_skipped_with_a_warning() {
        let bad = ASSISTANT.replace("\"inputTokens\":14607", "\"inputTokens\":\"x\"");
        let (home, _) = write_session(&[HEADER, &bad, ASSISTANT2]);
        let r = CommandcodeReader.read(&ReaderContext::new(&home));
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.events[0].message_id, "a2");
        assert!(r.warnings[0].message.contains("malformed usage"));
    }

    #[test]
    fn missing_model_falls_back_to_unknown() {
        let no_model = ASSISTANT.replace(",\"model\":\"llama/qwen\"", "");
        let (home, _) = write_session(&[HEADER, &no_model]);
        let r = CommandcodeReader.read(&ReaderContext::new(&home));
        assert_eq!(r.events[0].model, UNKNOWN_MODEL);
    }
}

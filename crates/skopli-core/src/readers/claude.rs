use std::path::{Path, PathBuf};

use serde_json::Value;

use super::reader::{Reader, ReaderContext};
use super::shared::{
    ReaderResult, ReaderWarning, as_string, dir_exists, finite_number, is_record, list_files,
    normalize_workspace, read_jsonl_lines,
};
use crate::types::{TokenCounts, UsageEvent};

/// The claude reader wired into the harness registry. Resolves its roots from
/// the `ReaderContext` via `claude_roots` (env override `CLAUDE_CONFIG_DIR`,
/// else the two `~/.claude` defaults), then delegates to
/// `read_claude_transcripts`.
pub struct ClaudeReader;

impl Reader for ClaudeReader {
    fn harness_id(&self) -> &'static str {
        "claude"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = claude_roots(ctx.env("CLAUDE_CONFIG_DIR"), ctx.home());
        read_claude_transcripts(&roots, "claude")
    }
}

/// Resolve claude data roots. Faithful port of `claudeRoots` in
/// src/readers/claude.ts: `CLAUDE_CONFIG_DIR` (comma-split, trimmed, non-empty
/// parts) wins; otherwise the two default `~/.claude` locations.
///
/// `env` returns the value of an environment variable (empty string treated as
/// unset by the caller, matching the TS PathResolver), `home` is the home dir.
pub fn claude_roots(config_dir: Option<&str>, home: &Path) -> Vec<PathBuf> {
    if let Some(override_val) = config_dir
        && !override_val.is_empty()
    {
        return override_val
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    vec![home.join(".claude"), home.join(".config").join("claude")]
}

/// True when a `user`-type line represents a human turn (not a tool result).
/// Faithful port of `isHumanUserLine`.
fn is_human_user_line(line: &Value) -> bool {
    if line.get("type").and_then(Value::as_str) != Some("user") {
        return false;
    }
    let message = match line.get("message") {
        Some(m) if is_record(m) => m,
        _ => return true,
    };
    let content = message.get("content");
    match content {
        Some(Value::String(_)) => true,
        Some(Value::Array(parts)) => !parts.iter().any(|part| {
            is_record(part) && part.get("type").and_then(Value::as_str) == Some("tool_result")
        }),
        _ => true,
    }
}

#[derive(PartialEq, Eq)]
enum LineRole {
    User,
    Assistant,
    Other,
}

struct ParseOutcome {
    event: Option<UsageEvent>,
    role: LineRole,
    malformed: bool,
}

/// Parse a single JSONL value into an optional event + role + malformed flag.
/// Faithful port of `parseLine` in src/readers/claude.ts.
fn parse_line(
    parsed: &Value,
    file: &str,
    index: usize,
    prev_was_user: bool,
    harness: &str,
) -> ParseOutcome {
    if !is_record(parsed) {
        return ParseOutcome {
            event: None,
            role: LineRole::Other,
            malformed: false,
        };
    }
    if is_human_user_line(parsed) {
        return ParseOutcome {
            event: None,
            role: LineRole::User,
            malformed: false,
        };
    }
    if parsed.get("type").and_then(Value::as_str) != Some("assistant") {
        return ParseOutcome {
            event: None,
            role: LineRole::Other,
            malformed: false,
        };
    }
    let message = match parsed.get("message") {
        Some(m) if is_record(m) => m,
        _ => {
            return ParseOutcome {
                event: None,
                role: LineRole::Assistant,
                malformed: true,
            };
        }
    };
    let usage = match message.get("usage") {
        Some(u) if is_record(u) => u,
        _ => {
            return ParseOutcome {
                event: None,
                role: LineRole::Assistant,
                malformed: true,
            };
        }
    };
    let model = message.get("model").and_then(as_string_val);
    // "<synthetic>" marks locally fabricated assistant rows with no API usage
    if model.as_deref() == Some("<synthetic>") {
        return ParseOutcome {
            event: None,
            role: LineRole::Assistant,
            malformed: false,
        };
    }
    let timestamp = parsed.get("timestamp").and_then(as_string_val);
    let input = usage.get("input_tokens").and_then(finite_val);
    let output = usage.get("output_tokens").and_then(finite_val);
    // present-but-not-finite -> None (malformed); absent -> Some(0)
    let cache_read = match usage.get("cache_read_input_tokens") {
        None => Some(0.0),
        Some(v) => finite_number(v),
    };
    let cache_write = match usage.get("cache_creation_input_tokens") {
        None => Some(0.0),
        Some(v) => finite_number(v),
    };
    // a missing or malformed 1h split only loses the rate refinement, not
    // usage, so it stays absent (all writes 5m) instead of rejecting the record
    let raw_write1h = match usage.get("cache_creation") {
        Some(c) if is_record(c) => c.get("ephemeral_1h_input_tokens").and_then(finite_number),
        _ => None,
    };
    let cache_write1h = raw_write1h.filter(|&n| n >= 0.0);

    let (model, timestamp, input, output, cache_read, cache_write) =
        match (model, timestamp, input, output, cache_read, cache_write) {
            (Some(m), Some(t), Some(i), Some(o), Some(cr), Some(cw)) => (m, t, i, o, cr, cw),
            _ => {
                return ParseOutcome {
                    event: None,
                    role: LineRole::Assistant,
                    malformed: true,
                };
            }
        };

    let message_id = message
        .get("id")
        .and_then(as_string_val)
        .or_else(|| parsed.get("requestId").and_then(as_string_val))
        .or_else(|| parsed.get("uuid").and_then(as_string_val))
        .unwrap_or_else(|| format!("{file}:{index}"));

    let cost_usd = parsed.get("costUSD").and_then(finite_number);
    let workspace = parsed.get("cwd").and_then(normalize_workspace);

    let event = UsageEvent {
        harness: harness.to_owned(),
        timestamp,
        session_id: parsed
            .get("sessionId")
            .and_then(as_string_val)
            .unwrap_or_else(|| file.to_owned()),
        message_id,
        turn: prev_was_user,
        subagent: parsed.get("isSidechain") == Some(&Value::Bool(true)),
        model,
        tokens: TokenCounts {
            input: input as u64,
            output: output as u64,
            cache_read: cache_read as u64,
            cache_write: cache_write as u64,
            cache_write1h: cache_write1h.map(|n| n as u64),
            reasoning: 0,
        },
        calls: None,
        cost_usd,
        workspace,
        title: None,
    };
    ParseOutcome {
        event: Some(event),
        role: LineRole::Assistant,
        malformed: false,
    }
}

fn as_string_val(v: &Value) -> Option<String> {
    as_string(v)
}

fn finite_val(v: &Value) -> Option<f64> {
    finite_number(v)
}

/// Read all Claude-Code-style transcript usage events from the given roots.
/// Faithful port of `readClaude` in src/readers/claude.ts: for each root, walk
/// `projects/`, read each `.jsonl`, track user->assistant turn state, warn+skip
/// malformed assistant records, and dedup globally per-harness on `messageId`
/// (first occurrence wins). `harness` parameterizes the emitted event's harness
/// literal so CodeBuddy Code (a Claude Code transcript clone) can reuse this
/// reader by swapping its home dir and id.
pub fn read_claude_transcripts(roots: &[PathBuf], harness: &str) -> ReaderResult {
    let mut events: Vec<UsageEvent> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<ReaderWarning> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in roots {
        let projects = root.join("projects");
        if !dir_exists(&projects) {
            continue;
        }
        for file in list_files(&projects, |name| name.ends_with(".jsonl")) {
            let lines = match read_jsonl_lines(&file, &mut skipped, &mut warnings) {
                Some(l) => l,
                None => continue,
            };
            let mut prev_was_user = false;
            for line in &lines {
                let outcome = parse_line(&line.value, &file, line.index, prev_was_user, harness);
                match outcome.role {
                    LineRole::User => prev_was_user = true,
                    LineRole::Assistant => prev_was_user = false,
                    LineRole::Other => {}
                }
                if outcome.malformed {
                    let location = format!("{file}:{}", line.index + 1);
                    warnings.push(ReaderWarning {
                        message: format!("skipping malformed assistant record {location}\n"),
                    });
                    skipped.push(location);
                }
                let event = match outcome.event {
                    Some(e) => e,
                    None => continue,
                };
                // global (per-harness) dedup: `claude --resume` copies history
                // into new session files, so the same message id across
                // sessions is a replay
                if seen.contains(&event.message_id) {
                    continue;
                }
                seen.insert(event.message_id.clone());
                events.push(event);
            }
        }
    }
    ReaderResult {
        events,
        skipped,
        warnings,
    }
}

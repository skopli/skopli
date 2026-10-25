use std::path::{Path, PathBuf};

use super::claude::read_claude_transcripts;
use super::reader::{Reader, ReaderContext};
use super::shared::ReaderResult;

/// The CodeBuddy Code reader wired into the harness registry. CodeBuddy Code
/// (Tencent) is a Claude Code transcript clone: same `<home>/projects/
/// <encoded-cwd>/<sessionId>.jsonl` layout and Anthropic usage field names, so
/// it reuses the Claude transcript reader with a `.codebuddy` home and the
/// `CODEBUDDY_CONFIG_DIR` env override. The distinct `codebuddy` id keeps it
/// separate from the unrelated `codebuff` reader.
pub struct CodebuddyReader;

impl Reader for CodebuddyReader {
    fn harness_id(&self) -> &'static str {
        "codebuddy"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = codebuddy_roots(ctx.env("CODEBUDDY_CONFIG_DIR"), ctx.home());
        read_claude_transcripts(&roots, "codebuddy")
    }
}

/// Resolve CodeBuddy Code data roots. A non-empty `CODEBUDDY_CONFIG_DIR` is
/// one home directory and wins; otherwise the default `~/.codebuddy` home.
pub fn codebuddy_roots(config_dir: Option<&str>, home: &Path) -> Vec<PathBuf> {
    if let Some(override_val) = config_dir
        && !override_val.is_empty()
    {
        return vec![PathBuf::from(override_val)];
    }
    vec![home.join(".codebuddy")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readers::shared::test_home;

    #[test]
    fn default_home_resolves_dot_codebuddy() {
        assert_eq!(
            codebuddy_roots(None, &test_home()),
            vec![test_home().join(".codebuddy")]
        );
    }

    #[test]
    fn config_dir_override_wins() {
        assert_eq!(
            codebuddy_roots(Some("/custom/cb"), &test_home()),
            vec![PathBuf::from("/custom/cb")]
        );
    }

    #[test]
    fn config_dir_with_comma_stays_one_path() {
        assert_eq!(
            codebuddy_roots(Some("/a, b"), &test_home()),
            vec![PathBuf::from("/a, b")]
        );
    }

    #[test]
    fn empty_override_falls_through_to_default() {
        assert_eq!(
            codebuddy_roots(Some(""), &test_home()),
            vec![test_home().join(".codebuddy")]
        );
    }

    // Current CodeBuddy rows: `type: "message"` with a top-level `role`, an epoch-ms
    // `timestamp`, the model under `providerData.model`, and the provider's raw usage
    // (OpenAI chat `prompt_tokens`, which includes the cached prefix) next to the
    // Anthropic-named `message.usage`.
    #[test]
    fn reads_message_rows_with_provider_data() {
        let root = crate::readers::shared::scratch_dir("codebuddy-rows");
        let dir = root.join("projects").join("home-user-project");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("s1.jsonl"),
            concat!(
                r#"{"id":"u1","timestamp":1792926532333,"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}],"sessionId":"s1"}"#,
                "\n",
                r#"{"id":"a1","parentId":"u1","timestamp":1792926948636,"type":"message","role":"assistant","providerData":{"model":"m1","rawUsage":{"completion_tokens":129,"prompt_tokens":22017,"prompt_tokens_details":{"cached_tokens":17944}}},"content":[{"type":"output_text","text":"ok"}],"sessionId":"s1","message":{"usage":{"input_tokens":22017,"output_tokens":129,"total_tokens":22146,"cache_read_input_tokens":17944}}}"#,
                "\n",
            ),
        )
        .unwrap();
        let result = read_claude_transcripts(std::slice::from_ref(&root), "codebuddy");
        assert_eq!(result.events.len(), 1, "{:?}", result.warnings);
        let e = &result.events[0];
        assert_eq!(e.timestamp, "2026-10-25T11:15:48.636Z");
        assert_eq!(e.model, "m1");
        assert_eq!(e.message_id, "a1");
        assert_eq!(e.session_id, "s1");
        assert!(e.turn);
        assert_eq!(
            (e.tokens.input, e.tokens.output, e.tokens.cache_read),
            (22017 - 17944, 129, 17944)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    // A tool-call step is written as `type: "function_call"` with no `role`; it carries
    // that request's usage like an assistant row and must be counted.
    #[test]
    fn function_call_rows_carry_usage() {
        let root = crate::readers::shared::scratch_dir("codebuddy-function-call");
        let dir = root.join("projects").join("home-user-project");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("s1.jsonl"),
            concat!(
                r#"{"id":"u1","timestamp":1792926532333,"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}],"sessionId":"s1"}"#,
                "\n",
                r#"{"id":"f1","parentId":"u1","timestamp":1792926948000,"type":"function_call","callId":"c1","name":"Read","arguments":"{}","providerData":{"model":"m1","rawUsage":{"completion_tokens":47,"prompt_tokens":22017,"prompt_tokens_details":{"cached_tokens":5}}},"sessionId":"s1","message":{"usage":{"input_tokens":22017,"output_tokens":47,"total_tokens":22064,"cache_read_input_tokens":5}}}"#,
                "\n",
                r#"{"id":"a1","parentId":"f1","timestamp":1792926948636,"type":"message","role":"assistant","providerData":{"model":"m1","rawUsage":{"completion_tokens":129,"prompt_tokens":22017,"prompt_tokens_details":{"cached_tokens":17944}}},"content":[{"type":"output_text","text":"ok"}],"sessionId":"s1","message":{"usage":{"input_tokens":22017,"output_tokens":129,"total_tokens":22146,"cache_read_input_tokens":17944}}}"#,
                "\n",
            ),
        )
        .unwrap();
        let result = read_claude_transcripts(std::slice::from_ref(&root), "codebuddy");
        assert_eq!(result.events.len(), 2, "{:?}", result.warnings);
        let f = &result.events[0];
        assert_eq!(f.message_id, "f1");
        assert_eq!(
            (f.tokens.input, f.tokens.output, f.tokens.cache_read),
            (22017 - 5, 47, 5)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}

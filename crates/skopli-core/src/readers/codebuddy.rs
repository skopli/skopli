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

    fn home() -> PathBuf {
        PathBuf::from("/home/u")
    }

    #[test]
    fn default_home_resolves_dot_codebuddy() {
        assert_eq!(
            codebuddy_roots(None, &home()),
            vec![home().join(".codebuddy")]
        );
    }

    #[test]
    fn config_dir_override_wins() {
        assert_eq!(
            codebuddy_roots(Some("/custom/cb"), &home()),
            vec![PathBuf::from("/custom/cb")]
        );
    }

    #[test]
    fn config_dir_with_comma_stays_one_path() {
        assert_eq!(
            codebuddy_roots(Some("/a, b"), &home()),
            vec![PathBuf::from("/a, b")]
        );
    }

    #[test]
    fn empty_override_falls_through_to_default() {
        assert_eq!(
            codebuddy_roots(Some(""), &home()),
            vec![home().join(".codebuddy")]
        );
    }
}

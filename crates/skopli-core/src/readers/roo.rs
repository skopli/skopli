use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::reader::{Reader, ReaderContext};
use super::shared::ReaderResult;
use super::vscode_ext::{read_vscode_ext, vscode_task_roots};

/// The roo reader wired into the harness registry. roo is a VS Code task-store
/// extension (`rooveterinaryinc.roo-cline`) sharing the per-OS globalStorage
/// layout with cline/kilocode.
pub struct RooReader;

impl Reader for RooReader {
    fn harness_id(&self) -> &'static str {
        "roo"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = roo_task_roots(ctx.env("APPDATA"), ctx.home());
        read_roo(&roots)
    }
}

/// Resolve the roo VS Code task roots.
pub fn roo_task_roots(app_data: Option<&str>, home: &Path) -> Vec<PathBuf> {
    vscode_task_roots(app_data, home, &["rooveterinaryinc.roo-cline", "tasks"])
}

/// Read all roo usage events.
pub fn read_roo(roots: &[PathBuf]) -> ReaderResult {
    let mut seen: HashSet<String> = HashSet::new();
    let mut covered_sessions: HashSet<String> = HashSet::new();
    read_vscode_ext("roo", roots, &mut seen, &mut covered_sessions)
}

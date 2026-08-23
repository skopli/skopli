use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::reader::{Reader, ReaderContext};
use super::shared::ReaderResult;
use super::vscode_ext::{read_vscode_ext, vscode_task_roots};

/// The kilocode reader wired into the harness registry. kilocode is a VS Code
/// task-store extension (`kilocode.kilo-code`) sharing the per-OS globalStorage
/// layout with roo/cline. Faithful port of `readKilocode` in roo-cline-kilo.ts.
pub struct KilocodeReader;

impl Reader for KilocodeReader {
    fn harness_id(&self) -> &'static str {
        "kilocode"
    }

    fn read(&self, ctx: &ReaderContext) -> ReaderResult {
        let roots = kilocode_task_roots(ctx.env("APPDATA"), ctx.home());
        read_kilocode(&roots)
    }
}

/// Resolve the kilocode VS Code task roots. Faithful port of `kilocodeTaskRoots`
/// -> `vscodeTaskRoots("kilocode")`.
pub fn kilocode_task_roots(app_data: Option<&str>, home: &Path) -> Vec<PathBuf> {
    vscode_task_roots(app_data, home, &["kilocode.kilo-code", "tasks"])
}

/// Read all kilocode usage events. Faithful port of `readKilocode` ->
/// `readVscodeExt("kilocode", roots)`.
pub fn read_kilocode(roots: &[PathBuf]) -> ReaderResult {
    let mut seen: HashSet<String> = HashSet::new();
    let mut covered_sessions: HashSet<String> = HashSet::new();
    read_vscode_ext("kilocode", roots, &mut seen, &mut covered_sessions)
}

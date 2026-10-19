//! Bounded enumeration of home layouts for the two detectors: the TypeScript
//! `detectHarnesses` in `src/detect.ts`, which probes roots and marker files,
//! and the Rust `skopli_wire::read::detect`, which marks a harness supported
//! when its reader yields an event. The marker table below is read off
//! `src/detect.ts` for a home with an empty environment and the home as the
//! working directory; a marker missing from the table is an uncovered probe.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::readers::{copy_tree, sorted_dirs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
}

/// One probe `src/detect.ts` makes under the home, and who it speaks for.
/// `None` for the harness means the marker names an unsupported product.
pub struct Marker {
    pub harness: Option<&'static str>,
    pub kind: Kind,
    pub path: &'static str,
}

const fn dir(harness: &'static str, path: &'static str) -> Marker {
    Marker {
        harness: Some(harness),
        kind: Kind::Dir,
        path,
    }
}

const fn file(harness: &'static str, path: &'static str) -> Marker {
    Marker {
        harness: Some(harness),
        kind: Kind::File,
        path,
    }
}

const fn unsupported(kind: Kind, path: &'static str) -> Marker {
    Marker {
        harness: None,
        kind,
        path,
    }
}

pub const MARKERS: &[Marker] = &[
    dir("claude", ".claude/projects"),
    dir("claude", ".config/claude/projects"),
    dir("opencode", ".local/share/opencode/storage"),
    file("opencode", ".local/share/opencode/opencode.db"),
    dir("mimocode", ".local/share/mimocode/storage"),
    file("mimocode", ".local/share/mimocode/opencode.db"),
    dir("commandcode", ".local/share/commandcode/storage"),
    dir(
        "commandcode",
        "Library/Application Support/CommandCode/storage",
    ),
    dir("codex", ".codex/sessions"),
    dir("codex", ".codex/archived_sessions"),
    dir("gemini", ".gemini/tmp"),
    dir("copilot", ".copilot/otel"),
    file("copilot", ".copilot/session-state/s1/events.jsonl"),
    dir("amp", ".local/share/amp/threads"),
    dir("droid", ".factory/sessions"),
    dir("qwen", ".qwen/projects"),
    dir("pi", ".pi/agent/sessions"),
    dir("omp", ".omp/agent/sessions"),
    dir("prime", ".prime/agent/sessions"),
    dir("gajae", ".gjc/agent/sessions"),
    dir("kimchi", ".config/kimchi/harness/sessions"),
    file("kilo", ".local/share/kilo/kilo.db"),
    file("goose", ".local/share/goose/sessions/sessions.db"),
    file("goose", ".local/share/Block/goose/sessions/sessions.db"),
    file(
        "goose",
        "Library/Application Support/goose/sessions/sessions.db",
    ),
    file("hermes", ".hermes/state.db"),
    file("hermes", ".hermes/profiles/p1/state.db"),
    dir("kimi", ".kimi/sessions"),
    dir("kimi", ".kimi-code/sessions"),
    dir("openclaw", ".openclaw"),
    dir("openclaw", ".clawdbot"),
    dir("openclaw", ".moltbot"),
    dir("openclaw", ".moldbot"),
    dir("junie", ".junie/sessions"),
    dir("grok", ".grok/sessions"),
    dir("grok", ".grok/logs"),
    dir("augment", ".augment/sessions"),
    dir("codebuff", ".config/manicode/projects"),
    dir("codebuff", ".config/manicode-dev/projects"),
    dir("codebuff", ".config/manicode-staging/projects"),
    dir("codebuddy", ".codebuddy/projects"),
    dir("jcode", ".jcode/sessions"),
    dir("mux", ".mux/sessions"),
    file("zed", ".local/share/zed/threads/threads.db"),
    file("zed", "Library/Application Support/Zed/threads/threads.db"),
    file(
        "cherrystudio",
        ".config/CherryStudio/Data/cherrystudio.sqlite",
    ),
    file(
        "cherrystudio",
        "Library/Application Support/CherryStudio/Data/cherrystudio.sqlite",
    ),
    file("zcode", ".zcode/cli/db/db.sqlite"),
    file("zcode", ".zcode/projects/p1/s1.jsonl"),
    file("devin", ".local/share/devin/cli/sessions.db"),
    file(
        "devin",
        "Library/Application Support/Devin/User/acp-events/e1.ndjson",
    ),
    dir(
        "roo",
        ".config/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
    ),
    dir(
        "roo",
        "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
    ),
    dir(
        "roo",
        ".vscode-server/data/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
    ),
    dir(
        "roo",
        "AppData/Roaming/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
    ),
    dir(
        "cline",
        ".config/Code/User/globalStorage/saoudrizwan.claude-dev/tasks",
    ),
    dir(
        "cline",
        "Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/tasks",
    ),
    dir(
        "cline",
        ".vscode-server/data/User/globalStorage/saoudrizwan.claude-dev/tasks",
    ),
    dir(
        "cline",
        "AppData/Roaming/Code/User/globalStorage/saoudrizwan.claude-dev/tasks",
    ),
    dir("cline", ".cline/data/sessions"),
    dir(
        "kilocode",
        ".config/Code/User/globalStorage/kilocode.kilo-code/tasks",
    ),
    dir(
        "kilocode",
        "Library/Application Support/Code/User/globalStorage/kilocode.kilo-code/tasks",
    ),
    dir(
        "kilocode",
        ".vscode-server/data/User/globalStorage/kilocode.kilo-code/tasks",
    ),
    dir(
        "kilocode",
        "AppData/Roaming/Code/User/globalStorage/kilocode.kilo-code/tasks",
    ),
    dir("opencodereview", ".opencodereview/sessions"),
    dir("trae", "trajectories"),
    file("deepseek", ".dsh/session.jsonl"),
    file("deepseek", ".dsh/w1/session.v4.jsonl.zstd"),
    file("deepseek", ".dsh/w1/a.db"),
    file("reasonix", ".reasonix/projects/p1/s1.acp.json"),
    file("kiro", ".kiro/data.sqlite3"),
    file("kiro", ".local/share/amazon-q/data.sqlite3"),
    file("kiro", "Library/Application Support/amazon-q/data.sqlite3"),
    file("fx", ".fx/sessions/s1/usage-v2.json"),
    unsupported(Kind::Dir, ".cursor"),
    unsupported(Kind::Dir, "AppData/Roaming/Cursor"),
    unsupported(Kind::File, ".aider.chat.history.md"),
    unsupported(Kind::File, ".local/share/crush/projects.json"),
    unsupported(Kind::File, "AppData/Local/crush/projects.json"),
    unsupported(Kind::Dir, ".config/gitlab/duo"),
];

/// Create the markers at `indexes` under `home`. Files hold one empty line so
/// a reader opening them meets a real, empty record stream.
pub fn build_layout(home: &Path, indexes: &[usize]) {
    for &i in indexes {
        let m = &MARKERS[i];
        let target = home.join(m.path);
        match m.kind {
            Kind::Dir => fs::create_dir_all(&target).unwrap(),
            Kind::File => {
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(&target, b"\n").unwrap();
            }
        }
    }
}

/// `{supported, unsupported}` sorted, so two detectors compare by equality.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detection {
    pub supported: Vec<String>,
    pub unsupported: Vec<String>,
}

impl Detection {
    pub fn from_value(v: &Value) -> Self {
        let list = |k: &str| {
            let mut out: Vec<String> = v[k]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_owned())
                .collect();
            out.sort();
            out
        };
        Detection {
            supported: list("supported"),
            unsupported: list("unsupported"),
        }
    }
}

pub struct Home {
    pub home: String,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
}

impl Home {
    pub fn bare(path: &Path) -> Self {
        let s = path.to_string_lossy().into_owned();
        Home {
            home: s.clone(),
            env: BTreeMap::new(),
            cwd: s,
        }
    }

    fn opts(&self) -> Value {
        json!({ "home": self.home, "env": self.env, "cwd": self.cwd })
    }
}

/// The TypeScript detector, one node process for the whole batch.
pub fn ts_detect<'a>(homes: impl IntoIterator<Item = &'a Home>) -> Vec<Detection> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ts-detect.mjs");
    let mut child = Command::new("node")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("node 24 or newer on PATH");
    let input: Vec<Value> = homes.into_iter().map(Home::opts).collect();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&input).unwrap().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "ts-detect.mjs failed");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(Detection::from_value)
        .collect()
}

/// The `<harness>/<endpoint>/` homes under a recordings root, labelled
/// `harness/endpoint`. A root that is missing or holds no home is a broken
/// replay, not a clean one.
pub fn recorded_homes(root: &Path) -> Vec<(String, Home)> {
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let mut homes = Vec::new();
    for harness in sorted_dirs(root) {
        for endpoint in sorted_dirs(&harness) {
            homes.push((
                format!("{}/{}", name(&harness), name(&endpoint)),
                Home::bare(&endpoint),
            ));
        }
    }
    assert!(
        !homes.is_empty(),
        "{} holds no <harness>/<endpoint>/ home",
        root.display()
    );
    homes
}

/// The Rust detector through the wire entry point the facades call.
pub fn rust_detect(home: &Home) -> Detection {
    Detection::from_value(&skopli_wire::read::detect(&home.opts()))
}

/// Where a golden case's data root sits in a bare home: the env variable the
/// manifest overrides, the sub path of the override the default root maps to,
/// and that default root relative to the home. Read off each reader's root
/// resolver in `crates/skopli-core/src/readers/`.
pub const DEFAULT_ROOTS: &[(&str, &str, &str)] = &[
    ("AMP_DATA_DIR", "", ".local/share/amp"),
    ("AUGMENT_SESSIONS_DIR", "", ".augment/sessions"),
    ("CLAUDE_CONFIG_DIR", "", ".claude"),
    ("CODEBUDDY_CONFIG_DIR", "", ".codebuddy"),
    ("CODEBUFF_DATA_DIR", "", ".config/manicode/projects"),
    ("CODEX_HOME", "", ".codex"),
    ("XDG_DATA_HOME", "", ".local/share"),
    ("COPILOT_OTEL_FILE_EXPORTER_PATH", "", ".copilot/otel"),
    ("DSH_HOME", "", ".dsh"),
    ("DEVIN_DATA_DIR", "", ".local/share/devin"),
    (
        "DEVIN_DESKTOP_DIR",
        "",
        "Library/Application Support/Devin/User/acp-events",
    ),
    ("DROID_SESSIONS_DIR", "", ".factory/sessions"),
    ("GJC_CODING_AGENT_DIR", "", ".gjc/agent"),
    ("GEMINI_DATA_DIR", "", ".gemini"),
    (
        "GOOSE_PATH_ROOT",
        "data/sessions",
        ".local/share/goose/sessions",
    ),
    ("GROK_HOME", "", ".grok"),
    ("HERMES_HOME", "", ".hermes"),
    ("JCODE_HOME", "", ".jcode"),
    ("JUNIE_SESSIONS_DIR", "", ".junie/sessions"),
    ("KILO_DATA_DIR", "", ".local/share/kilo"),
    ("KIMCHI_CODING_AGENT_DIR", "", ".config/kimchi/harness"),
    ("KIMI_DATA_DIR", "", ".kimi"),
    ("MIMOCODE_HOME", "data", ".local/share/mimocode"),
    ("MUX_SESSIONS_DIR", "", ".mux/sessions"),
    ("OPENCLAW_DIR", "", ".openclaw"),
    ("OPENCODE_DATA_DIR", "", ".local/share/opencode"),
    ("PI_AGENT_DIR", "", ".pi/agent"),
    ("PRIME_AGENT_SESSION_DIR", "", ".prime/agent/sessions"),
    ("QWEN_DATA_DIR", "", ".qwen"),
    ("REASONIX_HOME", "", ".reasonix"),
    ("ZCODE_DATA_DIR", "", ".zcode"),
    ("ZED_DATA_DIR", "", ".local/share/zed"),
];

/// Sibling roots both detectors probe for the same data on another operating
/// system: the Linux default a golden home is seeded at, and the macOS,
/// VS Code Server or Windows path the same tree is found at instead.
pub const SIBLING_ROOTS: &[(&str, &str)] = &[
    (
        ".config/Code/User/globalStorage",
        "Library/Application Support/Code/User/globalStorage",
    ),
    (
        ".config/Code/User/globalStorage",
        ".vscode-server/data/User/globalStorage",
    ),
    (
        ".config/Code/User/globalStorage",
        "AppData/Roaming/Code/User/globalStorage",
    ),
    (
        ".config/CherryStudio/Data/cherrystudio.sqlite",
        "Library/Application Support/CherryStudio/Data/cherrystudio.sqlite",
    ),
    (
        ".local/share/commandcode",
        "Library/Application Support/CommandCode",
    ),
    (
        ".local/share/goose/sessions/sessions.db",
        "Library/Application Support/goose/sessions/sessions.db",
    ),
    (
        ".local/share/goose/sessions/sessions.db",
        ".local/share/Block/goose/sessions/sessions.db",
    ),
    (".kiro/data.sqlite3", ".local/share/amazon-q/data.sqlite3"),
    (
        ".kiro/data.sqlite3",
        "Library/Application Support/amazon-q/data.sqlite3",
    ),
    (
        ".local/share/zed/threads/threads.db",
        "Library/Application Support/Zed/threads/threads.db",
    ),
];

/// Copy `home` to `into` with the tree at `from` moved to `to`.
pub fn relocate(home: &Path, into: &Path, from: &str, to: &str) {
    copy_tree(home, into);
    let target = into.join(to);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::rename(into.join(from), target).unwrap();
}

/// Copy a golden case into `home` where its default root would find it, with
/// no environment override. Cases whose manifest sets `home` or `cwd` to the
/// input are copied as the home itself.
pub fn seed_default_root(case: &crate::readers::Case, home: &Path) {
    let input = case.dir.join("input");
    let env = case.manifest["env"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if env.is_empty() {
        copy_tree(&input, home);
        return;
    }
    for (var, value) in &env {
        let (_, sub, root) = DEFAULT_ROOTS
            .iter()
            .find(|(v, _, _)| v == var)
            .unwrap_or_else(|| panic!("{}: no default root for {var}", case.id()));
        let value = value.as_str().unwrap();
        let mut src = if value == "." {
            input.clone()
        } else {
            input.join(value)
        };
        if !sub.is_empty() {
            src = src.join(sub);
        }
        copy_tree(&src, &home.join(root));
    }
}

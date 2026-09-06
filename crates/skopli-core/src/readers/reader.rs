use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::shared::ReaderResult;

/// Ambient inputs a reader needs to resolve its data roots. Mirrors the TS
/// `PathResolver` (src/paths.ts): a `home` directory and an `env` lookup where
/// an empty-string value is treated as unset. When an explicit `env` map is
/// provided it is the entire environment (the process environment is never
/// consulted), so tests and relocated setups get full isolation.
///
/// This is the Rust analogue of the reader-facing path-resolution surface the
/// ABI supplies: a `home_dir` plus the env/override map that drives each
/// reader's env-override roots (e.g. `CLAUDE_CONFIG_DIR`).
#[derive(Debug, Clone)]
pub struct ReaderContext {
    home: PathBuf,
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
}

impl ReaderContext {
    /// A context with the given home directory, no env overrides, and no working
    /// directory. `cwd` is opt-in (see [`with_cwd`](Self::with_cwd)); readers
    /// that resolve project-local roots from the working directory (e.g.
    /// trae-agent, whose trajectory files live under CWD) yield nothing when it
    /// is absent, so a context built without a cwd stays fully backward
    /// compatible for every home/env reader.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            env: BTreeMap::new(),
            cwd: None,
        }
    }

    /// Set an environment override. Empty values are treated as unset, matching
    /// the TS resolver, so `env("X")` returns `None` for an empty override.
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Set the working directory used by project-local readers. Injectable in
    /// tests; defaulted from the process working directory at the production
    /// call site.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// The home directory.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The working directory, when one was supplied. `None` for a context built
    /// without [`with_cwd`](Self::with_cwd); project-local readers treat that as
    /// "no data reachable" rather than an error.
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Look up an environment variable. An empty-string value is treated as
    /// unset (returns `None`), matching the TS `PathResolver`.
    pub fn env(&self, name: &str) -> Option<&str> {
        match self.env.get(name) {
            Some(v) if !v.is_empty() => Some(v.as_str()),
            _ => None,
        }
    }
}

/// A usage reader for a single harness. Adding a reader = implementing this
/// trait and registering it in the harness registry.
///
/// A reader resolves its own data roots from the `ReaderContext` (env-override
/// path resolution, exactly like the TS readers), then reads and returns a
/// `ReaderResult` carrying events + skipped + warnings (diagnostics). The
/// harness/exporter owns the downstream normalization (relativization, the
/// canonical event sort, and the gold envelope shape).
pub trait Reader {
    /// The harness id this reader produces events for (e.g. `"claude"`). Used
    /// as the `golden/<harness>/` directory name and the diagnostics `harness`
    /// literal.
    fn harness_id(&self) -> &'static str;

    /// Read all usage events reachable from the roots the context resolves.
    fn read(&self, ctx: &ReaderContext) -> ReaderResult;

    /// Whether this reader can serve the case rooted at `input_dir`. Multi-lane
    /// harnesses (e.g. opencode ships a JSON lane and a SQLite lane) override
    /// this so a case whose format is not yet ported is reported as pending
    /// rather than failed. Defaults to `true`.
    fn supports(&self, _input_dir: &Path) -> bool {
        true
    }
}

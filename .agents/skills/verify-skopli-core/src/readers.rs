//! Bounded mutation check of the readers in `crates/skopli-core/src/readers/`.
//! Every golden case is copied to a scratch home, each JSON leaf is replaced by
//! each adversarial value and each file is cut at fixed points, and the real
//! reader is run through `Reader::read` on the result. Expectations come from
//! the golden corpus and from the cell names, never from the crate under test.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use serde_json::Value;
use skopli_core::readers::reader::ReaderContext;
use skopli_core::readers::registered_reader;
use skopli_core::readers::shared::ReaderResult;
use skopli_core::types::UsageEvent;

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("skill lives three levels under the repository root")
}

/// One `golden/<harness>/<case>/` directory with an `input-manifest.json`.
pub struct Case {
    pub harness: String,
    pub name: String,
    pub dir: PathBuf,
    pub manifest: Value,
}

impl Case {
    pub fn id(&self) -> String {
        format!("{}/{}", self.harness, self.name)
    }

    pub fn expected_events(&self) -> Vec<Value> {
        let text = fs::read_to_string(self.dir.join("expected-events.json")).unwrap();
        serde_json::from_str::<Value>(&text).unwrap()["events"]
            .as_array()
            .unwrap()
            .clone()
    }
}

pub fn golden_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for harness in sorted_dirs(&repo_root().join("golden")) {
        for dir in sorted_dirs(&harness) {
            let Ok(text) = fs::read_to_string(dir.join("input-manifest.json")) else {
                continue;
            };
            let manifest: Value = serde_json::from_str(&text).unwrap();
            cases.push(Case {
                harness: manifest["harness"].as_str().unwrap().to_owned(),
                name: manifest["case"].as_str().unwrap().to_owned(),
                dir,
                manifest,
            });
        }
    }
    cases
}

pub fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// A fresh scratch directory under the system temp dir, removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Self {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("verify-skopli-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().filter_map(Result::ok) {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A case copied under a scratch directory: the copy, the reader context the
/// manifest describes and the same context as wire read options.
pub struct Seed {
    pub input: PathBuf,
    pub ctx: ReaderContext,
    pub opts: Value,
}

/// Copy the case under `scratch`. `env` values of `.` are the input copy,
/// other relative values are joined onto it, `home` and `cwd` of `.` are the
/// copy, and an absent home is a path that does not exist so no default root
/// resolves.
pub fn seed(case: &Case, scratch: &Path) -> Seed {
    let input = scratch.join("input");
    copy_tree(&case.dir.join("input"), &input);
    let resolve = |v: &str| -> String {
        let p = if v == "." {
            input.clone()
        } else if Path::new(v).is_absolute() {
            PathBuf::from(v)
        } else {
            input.join(v)
        };
        p.to_string_lossy().into_owned()
    };
    let home = case.manifest["home"]
        .as_str()
        .map(resolve)
        .unwrap_or_else(|| scratch.join("no-home").to_string_lossy().into_owned());
    let mut env = serde_json::Map::new();
    let mut ctx = ReaderContext::new(&home);
    for (name, value) in case.manifest["env"].as_object().into_iter().flatten() {
        let resolved = resolve(value.as_str().unwrap());
        ctx = ctx.with_env(name, &resolved);
        env.insert(name.clone(), Value::from(resolved));
    }
    let cwd = case.manifest["cwd"].as_str().map(resolve);
    if let Some(cwd) = &cwd {
        ctx = ctx.with_cwd(cwd);
    }
    let opts = serde_json::json!({
        "home": home,
        "env": env,
        "cwd": cwd.unwrap_or_default(),
        "harnesses": [case.harness],
    });
    Seed { input, ctx, opts }
}

/// The wire read the facades call, on the same home: events kept and the
/// diagnostics raised.
pub fn wire_read(opts: &Value) -> (usize, Vec<String>) {
    let options = skopli_wire::read::parse_read_options(opts).unwrap();
    let out = skopli_wire::read::read_usage(&options);
    let diagnostics = out["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["message"].as_str().unwrap_or_default().to_owned())
        .collect();
    (out["events"].as_array().unwrap().len(), diagnostics)
}

/// Run the registered reader; `Err` carries the panic message.
pub fn read(harness: &str, ctx: &ReaderContext) -> Result<ReaderResult, String> {
    let reader = registered_reader(harness).expect("every golden harness has a reader");
    catch_unwind(AssertUnwindSafe(|| reader.read(ctx))).map_err(|p| {
        p.downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "non-string panic payload".to_owned())
    })
}

pub fn diagnostics(r: &ReaderResult) -> usize {
    r.skipped.len() + r.warnings.len()
}

/// Every file under `dir`, sorted, as paths relative to it.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap().filter_map(Result::ok) {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

/// A JSON document inside a file: the whole file for `.json`, one line for
/// `.jsonl`. `line` is the 0-based line index, `None` for a whole file.
pub struct Doc {
    pub line: Option<usize>,
    pub value: Value,
}

/// The documents in `text`, the contents of `path`; empty for files that are
/// not JSON.
pub fn docs(path: &Path, text: &str) -> Vec<Doc> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => serde_json::from_str(text)
            .map(|value| vec![Doc { line: None, value }])
            .unwrap_or_default(),
        Some("jsonl" | "ndjson") => text
            .split('\n')
            .enumerate()
            .filter_map(|(i, l)| {
                serde_json::from_str(l.trim()).ok().map(|value| Doc {
                    line: Some(i),
                    value,
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Write `doc` back into `path`, replacing only its own line in a JSONL file.
pub fn write_doc(path: &Path, original: &str, doc: &Doc) {
    let text = match doc.line {
        None => serde_json::to_string(&doc.value).unwrap(),
        Some(i) => {
            let mut lines: Vec<String> = original.split('\n').map(str::to_owned).collect();
            lines[i] = serde_json::to_string(&doc.value).unwrap();
            lines.join("\n")
        }
    };
    fs::write(path, text).unwrap();
}

/// Remove `doc` from its file: its own line in a JSONL file, the whole file
/// otherwise.
pub fn delete_doc(path: &Path, original: &str, doc: &Doc) {
    match doc.line {
        None => fs::remove_file(path).unwrap(),
        Some(i) => {
            let lines: Vec<&str> = original
                .split('\n')
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, l)| l)
                .collect();
            fs::write(path, lines.join("\n")).unwrap();
        }
    }
}

/// Every leaf path in `value`, as a list of object keys and array indexes.
pub fn leaves(value: &Value) -> Vec<Vec<String>> {
    fn walk(v: &Value, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        match v {
            Value::Object(m) if !m.is_empty() => {
                for (k, child) in m {
                    prefix.push(k.clone());
                    walk(child, prefix, out);
                    prefix.pop();
                }
            }
            Value::Array(a) if !a.is_empty() => {
                for (i, child) in a.iter().enumerate() {
                    prefix.push(i.to_string());
                    walk(child, prefix, out);
                    prefix.pop();
                }
            }
            _ => out.push(prefix.clone()),
        }
    }
    let mut out = Vec::new();
    walk(value, &mut Vec::new(), &mut out);
    out
}

/// The record a leaf belongs to: the deepest array element on its path, or
/// the whole document when the path crosses no array.
pub fn record_of(leaf: &[String]) -> &[String] {
    let end = leaf
        .iter()
        .rposition(|k| k.parse::<usize>().is_ok())
        .map_or(0, |i| i + 1);
    &leaf[..end]
}

/// Bump the cell at `leaf` by one and keep a sibling `total` cell consistent
/// when the class is part of a total. The gemini reader reads the total to
/// decide whether the input includes the cached tokens, so a bump that breaks
/// the total would flip that reading instead of testing the mapping.
pub fn bump(value: &mut Value, leaf: &[String], class: TokenClass) {
    let n = integer_cell(get_mut(value, leaf)).unwrap();
    *get_mut(value, leaf) = Value::from(n + 1);
    if matches!(
        class,
        TokenClass::CacheRead | TokenClass::CacheWrite | TokenClass::CacheWrite1h
    ) {
        return;
    }
    let (_, parent) = leaf.split_last().unwrap();
    if let Value::Object(m) = get_mut(value, parent) {
        for (_, v) in m
            .iter_mut()
            .filter(|(k, _)| k.to_ascii_lowercase().contains("total"))
        {
            if let Some(t) = integer_cell(v) {
                *v = Value::from(t + 1);
            }
        }
    }
}

pub fn get<'a>(value: &'a Value, path: &[String]) -> &'a Value {
    path.iter().fold(value, |v, k| match v {
        Value::Array(a) => &a[k.parse::<usize>().unwrap()],
        Value::Object(m) => &m[k],
        _ => unreachable!("leaf paths only descend through containers"),
    })
}

pub fn get_mut<'a>(value: &'a mut Value, path: &[String]) -> &'a mut Value {
    path.iter().fold(value, |v, k| match v {
        Value::Array(a) => &mut a[k.parse::<usize>().unwrap()],
        Value::Object(m) => m.get_mut(k).unwrap(),
        _ => unreachable!("leaf paths only descend through containers"),
    })
}

pub fn remove(value: &mut Value, path: &[String]) {
    let (last, parent) = path.split_last().unwrap();
    match get_mut(value, parent) {
        Value::Array(a) => {
            a.remove(last.parse().unwrap());
        }
        Value::Object(m) => {
            m.remove(last);
        }
        _ => unreachable!(),
    }
}

/// Values a producer could never legitimately write into a cell, plus the
/// cell's removal (`None`).
pub fn adversarial_values() -> Vec<(&'static str, Option<Value>)> {
    vec![
        ("null", Some(Value::Null)),
        ("empty string", Some(Value::from(""))),
        ("text", Some(Value::from("x"))),
        ("negative", Some(Value::from(-1))),
        ("huge", Some(Value::from(1e300))),
        ("past u64", Some(Value::from(18_446_744_073_709_551_616.0))),
        ("bool", Some(Value::Bool(true))),
        ("object", Some(serde_json::json!({}))),
        ("array", Some(serde_json::json!([]))),
        ("removed", None),
    ]
}

/// Byte-level damage applied to every file, JSON or not.
pub fn damaged(bytes: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("empty", Vec::new()),
        ("first byte", bytes[..bytes.len().min(1)].to_vec()),
        ("first half", bytes[..bytes.len() / 2].to_vec()),
        (
            "last byte cut",
            bytes[..bytes.len().saturating_sub(1)].to_vec(),
        ),
        ("open brace", b"{".to_vec()),
        ("binary", vec![0, 0xff, 0xfe, b'{', b'"']),
    ]
}

/// The counter a token cell name speaks for, read from the cell path alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    Input,
    Output,
    CacheRead,
    CacheWrite,
    CacheWrite1h,
    Reasoning,
}

impl TokenClass {
    /// `None` for cells that are not token counters or are derived totals.
    /// A cell named `tokens` is classified by its parent (`input.tokens`).
    pub fn of(path: &[String]) -> Option<Self> {
        let p = path.join(".").to_ascii_lowercase();
        let mut names = path.iter().rev().map(|s| s.to_ascii_lowercase());
        let mut last = names.next().unwrap_or_default();
        if last == "tokens" {
            last = names.next().unwrap_or_default();
        }
        if p.contains("total") || last.contains("cost") || last.contains("price") {
            return None;
        }
        if last.contains("reason") || last.contains("thought") {
            Some(Self::Reasoning)
        } else if last.contains("cachemiss") {
            Some(Self::Input)
        } else if p.contains("cache") {
            Some(if last.contains("1h") {
                Self::CacheWrite1h
            } else if p.contains("write") || p.contains("creat") {
                Self::CacheWrite
            } else {
                Self::CacheRead
            })
        } else if last.contains("input") || last.contains("prompt") {
            Some(Self::Input)
        } else if last.contains("output")
            || last.contains("completion")
            || last.contains("candidates")
        {
            Some(Self::Output)
        } else {
            None
        }
    }

    /// The event counter a cell of this class speaks for; a bump must move it
    /// by `+1`.
    pub fn own(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::CacheRead => "cacheRead",
            Self::CacheWrite => "cacheWrite",
            Self::CacheWrite1h => "cacheWrite1h",
            Self::Reasoning => "reasoning",
        }
    }

    /// The one counter a reader derives by subtracting this class (prompt
    /// minus cached, completion minus reasoning); a bump may move it by `-1`.
    pub fn derived(self) -> Option<&'static str> {
        match self {
            Self::CacheRead | Self::CacheWrite | Self::CacheWrite1h => Some("input"),
            Self::Reasoning => Some("output"),
            Self::Input | Self::Output => None,
        }
    }
}

pub const COUNTERS: [&str; 6] = [
    "input",
    "output",
    "cacheRead",
    "cacheWrite",
    "cacheWrite1h",
    "reasoning",
];

/// One event's counters in `COUNTERS` order.
pub fn counters(e: &UsageEvent) -> [i128; 6] {
    let k = &e.tokens;
    [
        k.input,
        k.output,
        k.cache_read,
        k.cache_write,
        k.cache_write1h.unwrap_or(0),
        k.reasoning,
    ]
    .map(i128::from)
}

/// The identity a reader emits for one event: timestamp, session, message
/// and model. Readers that walk hash maps emit in no fixed order, so the
/// mapping checks sort by this instead of trusting emission order.
pub type Identity = (String, String, String, String);

/// The event-level view the two mapping checks compare: every event's
/// identity and counters, sorted by identity, and the diagnostic count.
pub struct Outcome {
    pub events: Vec<(Identity, [i128; 6])>,
    pub diagnostics: usize,
}

impl Outcome {
    pub fn of(r: &ReaderResult) -> Self {
        let mut events: Vec<_> = r
            .events
            .iter()
            .map(|e| {
                let id = (
                    e.timestamp.clone(),
                    e.session_id.clone(),
                    e.message_id.clone(),
                    e.model.clone(),
                );
                (id, counters(e))
            })
            .collect();
        events.sort();
        Outcome {
            events,
            diagnostics: diagnostics(r),
        }
    }
}

/// A failed verdict: the failure class and the counterexample.
pub type Fault = (&'static str, String);

/// The verdict for one token cell bumped by `+1`: exactly one event changes
/// and keeps its identity, its class's own counter moves by `+1`, at most its
/// derived counter moves by `-1`, or the record becomes a diagnostic.
pub fn judge_bump(class: TokenClass, base: &Outcome, bumped: &Outcome) -> Result<(), Fault> {
    if bumped.events.len() < base.events.len() {
        return if bumped.diagnostics > base.diagnostics {
            Ok(())
        } else {
            Err((
                "silent drop",
                "the bumped record vanished without a diagnostic".to_owned(),
            ))
        };
    }
    // The bump may change one event's counters in place or surface one event
    // the reader dropped as all-zero; any other difference is misattribution.
    let (b, a) = (&base.events, &bumped.events);
    let prefix = b.iter().zip(a).take_while(|(x, y)| x == y).count();
    let suffix = b[prefix..]
        .iter()
        .rev()
        .zip(a[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (before, after) = (&b[prefix..b.len() - suffix], &a[prefix..a.len() - suffix]);
    let same_record = match (before, after) {
        ([], [_]) | ([], []) => true,
        ([(x, _)], [(y, _)]) => x == y,
        _ => false,
    };
    if !same_record {
        return Err((
            "cross-event attribution",
            format!("one cell bumped by 1 turned {before:?} into {after:?}"),
        ));
    }
    let moved: Vec<(&str, i128)> = COUNTERS
        .iter()
        .zip(before.first().map_or(&[0; 6], |(_, c)| c))
        .zip(after.first().map_or(&[0; 6], |(_, c)| c))
        .filter(|((_, a), b)| a != b)
        .map(|((name, a), b)| (*name, b - a))
        .collect();
    if moved.is_empty() {
        return if bumped.diagnostics > base.diagnostics {
            Ok(())
        } else {
            Err((
                "ignored cell",
                "the bump moved no counter and raised no diagnostic".to_owned(),
            ))
        };
    }
    let own = class.own();
    for &(name, delta) in &moved {
        let want = if name == own {
            1
        } else if class.derived() == Some(name) {
            -1
        } else {
            0
        };
        if delta != want {
            return Err((
                if name == own {
                    "scaled"
                } else {
                    "dimension swap"
                },
                format!("a {class:?} cell bumped by 1 moved {name} by {delta}; moved {moved:?}"),
            ));
        }
    }
    if moved.iter().all(|&(name, _)| name != own) {
        return Err((
            "dimension swap",
            format!("a {class:?} cell bumped by 1 left {own} unmoved; moved {moved:?}"),
        ));
    }
    Ok(())
}

/// The verdict for a token cell overwritten with text: the record is kept or
/// becomes a diagnostic, never silently lost.
pub fn judge_corruption(base: &Outcome, corrupted: &Outcome) -> Result<(), Fault> {
    if corrupted.events.len() < base.events.len() && corrupted.diagnostics <= base.diagnostics {
        return Err((
            "silent drop",
            "a corrupted token cell lost the record without a diagnostic".to_owned(),
        ));
    }
    Ok(())
}

pub fn integer_cell(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
}

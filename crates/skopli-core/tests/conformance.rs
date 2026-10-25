//! Structural-parity conformance test, generalized over harness name.
//!
//! For each `golden/<harness>/<case>/`, look up the reader registered for
//! `<harness>` in the core registry. If one is registered, run it against
//! `<case>/input/` (env overrides from `input-manifest.json`) and structurally
//! compare the produced envelope to `expected-events.json` per the
//! structural-parity contract:
//!   - parsed-equal semantics (object key order ignored);
//!   - token counts as u64 exact; costUsd as f64 bit-exact;
//!   - an absent field is NOT equal to a null field;
//!   - events compared in the canonical sort order
//!     `(timestamp, sessionId, messageId, model)`, byte-wise.
//!
//! A `golden/<harness>/` directory with NO registered Rust reader is reported
//! as PENDING (skipped) - never a panic or failure - so gold for not-yet-ported
//! harnesses can land ahead of the reader.
//!
//! The harness reproduces the exporter's OS-independence normalization:
//! diagnostic/skip paths are relativized to the case `input/` root and use `/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::{Value, json};
use skopli_core::readers::reader::{Reader, ReaderContext};
use skopli_core::readers::shared::{ReaderResult, ReaderWarning};
use skopli_core::readers::{registered_reader, registered_readers};
use skopli_core::rollup::{RollupBy, RollupOptions, rollup};
use skopli_core::types::UsageEvent;

mod support;
use support::structural_eq;

/// Strip the absolute `input/` prefix (in `/` and `\` forms) from a path or
/// message and normalize `\` to `/`, matching the TS exporter: strip every bare
/// `inputDir` variant, normalize `\`->`/`, then collapse a run of leading
/// separators only at the start of the string or right after whitespace.
/// Unlike a naive prefix-with-separator strip, this preserves a separator that
/// follows a non-space, non-start character (e.g. the `/` in
/// `devin-desktop:/session-a.ndjson`), exactly as the exporter does.
fn relativize(text: &str, input_dir: &Path) -> String {
    let abs = input_dir.to_string_lossy();
    let abs_fwd = abs.replace('\\', "/");
    let abs_bwd = abs.replace('/', "\\");
    let mut out = text.to_owned();
    for variant in [abs.to_string(), abs_fwd, abs_bwd] {
        if variant.is_empty() {
            continue;
        }
        while out.contains(&variant) {
            out = out.replacen(&variant, "", 1);
        }
    }
    let out = out.replace('\\', "/");
    collapse_leading_separators(&out)
}

/// Collapse a run of leading `/` at the start of the string or immediately
/// after whitespace, matching the TS regex `/(^|\s)\/+/g` -> `$1`.
fn collapse_leading_separators(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_is_boundary = true;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if prev_is_boundary && c == '/' {
            while chars.peek() == Some(&'/') {
                chars.next();
            }
            prev_is_boundary = false;
            continue;
        }
        out.push(c);
        prev_is_boundary = c.is_whitespace();
    }
    out
}

/// Build the `expected-events.json`-shaped envelope from the reader result,
/// applying the canonical event sort and the diagnostics/skipped normalization
/// the exporter uses. `harness` parameterizes the diagnostics literal and the
/// skipped grouping key (was hard-coded `"claude"`).
/// The canonical event sort the exporter applies before writing gold files and
/// before rolling up: ascending byte-wise `(timestamp, sessionId, messageId,
/// model)`.
fn sort_events(events: &[UsageEvent]) -> Vec<UsageEvent> {
    let mut events = events.to_vec();
    events.sort_by(|a, b| {
        (&a.timestamp, &a.session_id, &a.message_id, &a.model).cmp(&(
            &b.timestamp,
            &b.session_id,
            &b.message_id,
            &b.model,
        ))
    });
    events
}

fn build_envelope(result: &ReaderResult, input_dir: &Path, harness: &str) -> Value {
    let events = sort_events(&result.events);
    let events_json: Vec<Value> = events
        .iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).expect("serialize event");
            // Some readers derive messageId or sessionId from the absolute
            // source path ("<file>:<line>"); rewrite the input-dir prefix to a
            // stable '/'-relative form so golden files carry no machine paths.
            if let Some(mid) = v.get("messageId").and_then(Value::as_str) {
                let rel = relativize(mid, input_dir);
                v["messageId"] = Value::String(rel);
            }
            if let Some(sid) = v.get("sessionId").and_then(Value::as_str) {
                let abs = input_dir.to_string_lossy();
                if sid.starts_with(abs.as_ref())
                    || sid.starts_with(&abs.replace('\\', "/"))
                    || sid.starts_with(&abs.replace('/', "\\"))
                {
                    v["sessionId"] = Value::String(relativize(sid, input_dir));
                }
            }
            v
        })
        .collect();

    // Diagnostics: one per warning, severity "warning", harness parameterized,
    // message relativized; sorted by (severity, message).
    let mut diagnostics: Vec<(String, String)> = result
        .warnings
        .iter()
        .map(|ReaderWarning { message }| {
            (
                "warning".to_owned(),
                relativize(message.trim_end_matches('\n'), input_dir),
            )
        })
        .collect();
    diagnostics.sort();
    let diagnostics_json: Vec<Value> = diagnostics
        .iter()
        .map(|(severity, message)| {
            json!({"severity": severity, "message": message, "harness": harness})
        })
        .collect();

    // skipped: relativized, sorted, grouped by harness.
    let mut skipped: Vec<String> = result
        .skipped
        .iter()
        .map(|s| relativize(s, input_dir))
        .collect();
    skipped.sort();
    // The exporter only records a harness key when that reader skipped at least
    // one path (Object.fromEntries over the non-empty skipped map); an empty
    // skip list yields `skipped: {}`, not `{ "<harness>": [] }`.
    let mut skipped_by_harness = serde_json::Map::new();
    if !skipped.is_empty() {
        skipped_by_harness.insert(harness.to_owned(), json!(skipped));
    }

    json!({
        "schema_version": 1,
        "events": events_json,
        "diagnostics": diagnostics_json,
        "skipped": Value::Object(skipped_by_harness),
    })
}

/// Build the `expected-rollup.json`-shaped envelope from the reader result:
/// `{schema_version, by: {<dim>: [Rollup, ...], ...}}`. `dimensions` are the
/// rollup lanes named in the case `input-manifest.json` (default
/// `["model", "day", "harness"]`). Day rollups always pass `tz: "UTC"`,
/// matching the exporter. Rollups run over the canonically-sorted events, as
/// the exporter does.
fn build_rollup_envelope(result: &ReaderResult, dimensions: &[String]) -> Value {
    let events = sort_events(&result.events);
    let mut by = serde_json::Map::new();
    for dim in dimensions {
        let Some(rollup_by) = RollupBy::parse(dim) else {
            continue;
        };
        let options = if rollup_by == RollupBy::Day {
            RollupOptions {
                by: rollup_by,
                tz: Some("UTC".to_owned()),
                block_ms: None,
            }
        } else {
            RollupOptions::new(rollup_by)
        };
        let rollups = rollup(&events, &options);
        let rollups_json: Vec<Value> = rollups
            .iter()
            .map(|r| serde_json::to_value(r).expect("serialize rollup"))
            .collect();
        by.insert(dim.clone(), Value::Array(rollups_json));
    }
    json!({
        "schema_version": 1,
        "by": Value::Object(by),
    })
}

/// A discovered golden case: `golden/<harness>/<case>/`.
struct GoldenCase {
    harness: String,
    case: String,
    dir: PathBuf,
}

/// `input-manifest.json`: the home, cwd and env wiring plus the rollup lanes a
/// case exercises. Every field the exporter writes is named here so a typo or
/// an unknown key fails the case instead of silently falling back to defaults.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    harness: String,
    case: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    home: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "options")]
    _options: IgnoredAny,
    rollups: Option<Vec<String>>,
}

fn manifest_for(case: &GoldenCase) -> Manifest {
    let path = case.dir.join("input-manifest.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let manifest: Manifest =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        manifest.schema_version,
        1,
        "{}: schema_version",
        path.display()
    );
    assert_eq!(
        manifest.harness,
        case.harness,
        "{}: harness",
        path.display()
    );
    assert_eq!(manifest.case, case.case, "{}: case", path.display());
    manifest
}

fn subdirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Every `golden/<harness>/<case>/` holding an `expected-events.json`, in a
/// stable order. `golden/pricing/`, `rollup-*` and `registry/` hold no cases.
fn discover_cases() -> Vec<GoldenCase> {
    let mut cases = Vec::new();
    for (harness, harness_dir) in subdirs(&support::repo_root().join("golden")) {
        for (case, dir) in subdirs(&harness_dir) {
            if dir.join("expected-events.json").is_file() {
                cases.push(GoldenCase {
                    harness: harness.clone(),
                    case,
                    dir,
                });
            }
        }
    }
    cases
}

/// Resolve the `ReaderContext` for a case with the same two rules as the TS
/// sweep: `home` and `cwd` are `"."` (the case `input/` dir) or taken verbatim;
/// an `env` value is joined onto `input/` unless absolute. Without `home` the
/// home is a nonexistent path so default roots never resolve; without `cwd` the
/// context carries none and project-local readers yield nothing.
fn context_for(dir: &Path, manifest: &Manifest) -> ReaderContext {
    let input_dir = dir.join("input");
    let at = |v: &str| {
        if v == "." {
            input_dir.clone()
        } else {
            PathBuf::from(v)
        }
    };
    let place = |v: &str| {
        if v == "." || Path::new(v).is_absolute() {
            at(v)
        } else {
            input_dir.join(v)
        }
    };
    let home = manifest
        .home
        .as_deref()
        .map(at)
        .unwrap_or_else(|| PathBuf::from("/nonexistent"));
    let mut ctx = ReaderContext::new(home);
    for (name, value) in &manifest.env {
        ctx = ctx.with_env(name.clone(), place(value).to_string_lossy().into_owned());
    }
    if let Some(cwd) = &manifest.cwd {
        ctx = ctx.with_cwd(at(cwd));
    }
    ctx
}

/// The registry-id fixture (`golden/registry/ids.json`) is the canonical id
/// list every SDK enum is checked against. This test keeps it honest: it must
/// list exactly the registered readers, in registered order, so a reader added
/// or reordered in the core without updating the fixture (and the SDK enums that
/// consume it) fails here.
#[test]
fn registry_id_fixture_matches_registered_readers() {
    let fixture = support::repo_root().join("golden/registry/ids.json");
    let parsed: Value = serde_json::from_str(
        &std::fs::read_to_string(&fixture).expect("read golden/registry/ids.json"),
    )
    .expect("parse golden/registry/ids.json");
    let fixture_ids: Vec<String> = parsed
        .get("ids")
        .and_then(Value::as_array)
        .expect("ids array")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let registered: Vec<String> = registered_readers()
        .iter()
        .map(|r| r.harness_id().to_owned())
        .collect();
    assert_eq!(
        fixture_ids, registered,
        "golden/registry/ids.json must match registered_readers() exactly (order included)"
    );
}

#[test]
fn conformance() {
    let cases = discover_cases();
    assert!(!cases.is_empty(), "no golden cases found under golden/");

    let mut passed: Vec<String> = Vec::new();
    let mut pending: BTreeMap<String, ()> = BTreeMap::new();
    let mut failures: BTreeMap<String, String> = BTreeMap::new();

    for case in &cases {
        let id = format!("{}/{}", case.harness, case.case);
        let Some(reader): Option<Box<dyn Reader>> = registered_reader(&case.harness) else {
            // No Rust reader wired for this harness yet: report pending, don't
            // fail. A gold dir can land ahead of its reader.
            println!(
                "PENDING {id} (no reader wired for harness \"{}\")",
                case.harness
            );
            pending.insert(case.harness.clone(), ());
            continue;
        };

        let input_dir = case.dir.join("input");

        // A registered reader may still not serve a particular case's format
        // (e.g. an unported input format). Report those as
        // pending rather than failing.
        if !reader.supports(&input_dir) {
            println!(
                "PENDING {id} (reader for harness \"{}\" does not serve this case format)",
                case.harness
            );
            pending.insert(case.harness.clone(), ());
            continue;
        }

        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(case.dir.join("expected-events.json"))
                .expect("read expected-events.json"),
        )
        .expect("parse expected-events.json");

        let manifest = manifest_for(case);
        let result = reader.read(&context_for(&case.dir, &manifest));
        let actual = build_envelope(&result, &input_dir, &case.harness);

        match structural_eq(&actual, &expected, "") {
            Ok(()) => {
                println!("PASS {id} (events)");
                passed.push(format!("{id} events"));
            }
            Err(diff) => {
                println!("FAIL {id} (events): {diff}");
                failures.insert(format!("{id} events"), diff);
            }
        }

        // Rollup conformance: compare the generated `by`-dimension envelope to
        // `expected-rollup.json` when present (every ported case has one).
        let rollup_path = case.dir.join("expected-rollup.json");
        if rollup_path.is_file() {
            let expected_rollup: Value = serde_json::from_str(
                &std::fs::read_to_string(&rollup_path).expect("read expected-rollup.json"),
            )
            .expect("parse expected-rollup.json");
            let dimensions = manifest.rollups.clone().unwrap_or_else(|| {
                vec!["model".to_owned(), "day".to_owned(), "harness".to_owned()]
            });
            let actual_rollup = build_rollup_envelope(&result, &dimensions);
            match structural_eq(&actual_rollup, &expected_rollup, "") {
                Ok(()) => {
                    println!("PASS {id} (rollup)");
                    passed.push(format!("{id} rollup"));
                }
                Err(diff) => {
                    println!("FAIL {id} (rollup): {diff}");
                    failures.insert(format!("{id} rollup"), diff);
                }
            }
        }
    }

    println!(
        "conformance: {} passed, {} pending harness(es), {} failed",
        passed.len(),
        pending.len(),
        failures.len()
    );
    for harness in pending.keys() {
        println!("  pending harness (no Rust reader): {harness}");
    }

    assert!(!passed.is_empty(), "no registered-harness cases ran");
    assert!(failures.is_empty(), "conformance failures: {failures:#?}");
}

#[test]
fn comparator_has_teeth() {
    let cases = [
        (
            json!({"a": 1}),
            json!({"a": 1, "b": null}),
            false,
            "absent is not null",
        ),
        (
            json!({"a": 1, "b": null}),
            json!({"a": 1}),
            false,
            "null is not absent",
        ),
        (json!(0.0123), json!(0.0124), false, "f64 mismatch detected"),
        (json!(3), json!(3.0), true, "integral int equals float"),
        (json!(0.0123), json!(0.0123), true, "equal f64 accepted"),
        (
            json!([1, 2]),
            json!([1, 2, 3]),
            false,
            "array length detected",
        ),
        (
            json!({"b": 2, "a": 1}),
            json!({"a": 1, "b": 2}),
            true,
            "key order ignored",
        ),
    ];
    for (actual, expected, should_match, name) in cases {
        assert_eq!(
            structural_eq(&actual, &expected, "").is_ok(),
            should_match,
            "{name}: {actual} vs {expected}"
        );
    }
}

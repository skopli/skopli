use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use skopli_core::rollup::{RollupBy, rollup};
use verify_skopli_core::*;

/// Read every JSON number as a float so a whole-number cost compares equal
/// whether it was written as `1.0` (Rust) or `1` (JavaScript).
fn as_floats(value: &Value) -> Value {
    match value {
        Value::Number(n) => Value::from(n.as_f64().unwrap()),
        Value::Array(items) => Value::Array(items.iter().map(as_floats).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), as_floats(v))).collect())
        }
        other => other.clone(),
    }
}

/// Group failures by the text before the first `:` and keep the first
/// counterexample of each class.
fn report(failures: Vec<(String, String)>, total: usize) {
    let mut first: BTreeMap<String, (String, String, usize)> = BTreeMap::new();
    for (message, case) in failures {
        let class = message.split(':').next().unwrap().to_owned();
        first
            .entry(class)
            .and_modify(|e| e.2 += 1)
            .or_insert((message, case, 1));
    }
    assert!(
        first.is_empty(),
        "{} of {total} cases failed:\n{}",
        first.values().map(|v| v.2).sum::<usize>(),
        first
            .iter()
            .map(|(class, (m, c, n))| format!("- {class} ({n} cases), first: {m}\n  {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn rollup_keeps_its_promises_on_every_case() {
    let cases = cases();
    let failures: Vec<(String, String)> = cases
        .iter()
        .filter_map(|(events, options)| {
            let out = rollup(events, options);
            check(events, options, &out).err().map(|m| {
                let rows = serde_json::to_string(&out).unwrap();
                (m, format!("{}\n  rows: {rows}", describe(events, options)))
            })
        })
        .collect();
    report(failures, cases.len());
}

/// Serialize the events with non-finite costs as the strings `"NaN"` and
/// `"Infinity"`, which JSON cannot carry as numbers; `ts-rollup.mjs` revives them.
fn events_json(events: &[skopli_core::types::UsageEvent]) -> Value {
    let mut values = serde_json::to_value(events).unwrap();
    for (value, event) in values.as_array_mut().unwrap().iter_mut().zip(events) {
        if let Some(cost) = event.cost_usd.filter(|c| !c.is_finite()) {
            value["costUsd"] = Value::from(if cost.is_nan() { "NaN" } else { "Infinity" });
        }
    }
    values
}

#[test]
fn rollup_agrees_with_the_typescript_facade() {
    let cases = cases();
    let input: Vec<Value> = cases
        .iter()
        .map(|(events, o)| {
            let mut options =
                serde_json::json!({ "by": format!("{:?}", o.by).to_lowercase(), "tz": o.tz });
            if let Some(ms) = o.block_ms {
                options["blockMs"] = Value::from(ms);
            }
            serde_json::json!({ "events": events_json(events), "options": options })
        })
        .collect();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ts-rollup.mjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node 24 must be on PATH");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let run = child.wait_with_output().unwrap();
    assert!(
        run.status.success(),
        "ts-rollup.mjs failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let ts: Vec<Value> = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(ts.len(), cases.len());
    let failures: Vec<(String, String)> = cases
        .iter()
        .zip(&ts)
        .filter_map(|((events, options), ts_out)| {
            let rust = serde_json::to_value(rollup(events, options)).unwrap();
            (as_floats(&rust) != as_floats(ts_out)).then(|| {
                (
                    format!("parity: rust and typescript differ for {:?}", options.by),
                    format!(
                        "{}\n  rust: {rust}\n  ts:   {ts_out}",
                        describe(events, options)
                    ),
                )
            })
        })
        .collect();
    report(failures, cases.len());
}

/// Known finding: the core sums `u64` counters unchecked, so two rows whose
/// counters together exceed `u64::MAX` panic in a debug build and wrap in a
/// release build, while the TypeScript facade keeps a (lossy) float total.
#[test]
fn counters_near_u64_max_do_not_wrap() {
    let mut big = PATTERNS[1];
    big.tokens = [u64::MAX, 0, 0, 0, 0];
    let events = [
        event("a", INSTANTS[0].iso, &big),
        event("a", INSTANTS[0].iso, &PATTERNS[1]),
    ];
    let out = rollup(&events, &opts(RollupBy::Model, "UTC", None));
    check_conservation(events.iter(), &out).unwrap_or_else(|m| panic!("{m}"));
    assert_eq!(
        out[0].tokens.input,
        u64::MAX,
        "the row saturates at the ceiling"
    );
}

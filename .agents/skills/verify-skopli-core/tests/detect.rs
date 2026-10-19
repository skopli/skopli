use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::catch_unwind;

use verify_skopli_core::detect::*;
use verify_skopli_core::readers::{Scratch, golden_cases, read};

/// The TypeScript detector over every layout with zero, one or two markers
/// present, plus all present: the verdict depends only on the layout, each
/// single marker is claimed by exactly the harness it was read off, and a pair
/// detects exactly the union of its singles. The Rust detector claims no
/// harness from an empty marker, so its verdict is a subset of TypeScript's
/// on every layout.
#[test]
fn layouts_detect_as_the_marker_table_says() {
    let n = MARKERS.len();
    let mut layouts: Vec<Vec<usize>> = vec![Vec::new(), (0..n).collect()];
    layouts.extend((0..n).map(|i| vec![i]));
    layouts.extend((0..n).flat_map(|i| (i + 1..n).map(move |j| vec![i, j])));

    let scratch = Scratch::new("layouts");
    let homes: Vec<Home> = layouts
        .iter()
        .enumerate()
        .map(|(k, idx)| {
            let home = scratch.0.join(format!("h{k}"));
            fs::create_dir_all(&home).unwrap();
            build_layout(&home, idx);
            Home::bare(&home)
        })
        .collect();
    let first = ts_detect(&homes);
    let second = ts_detect(&homes);
    assert_eq!(
        first, second,
        "detection is not a pure function of the layout"
    );

    let expected = |idx: &[usize]| -> Detection {
        let mut d = Detection::default();
        for &i in idx {
            match MARKERS[i].harness {
                Some(h) => d.supported.push(h.to_owned()),
                None => d.unsupported.push(unsupported_name(MARKERS[i].path)),
            }
        }
        d.supported.sort();
        d.supported.dedup();
        d.unsupported.sort();
        d.unsupported.dedup();
        d
    };
    let mut failures = Vec::new();
    for ((idx, got), home) in layouts.iter().zip(&first).zip(&homes) {
        let names: Vec<&str> = idx.iter().map(|&i| MARKERS[i].path).collect();
        let want = expected(idx);
        if *got != want {
            failures.push(format!("{names:?}: expected {want:?}, got {got:?}"));
        }
        let rust = rust_detect(home).supported;
        if !rust.is_empty() {
            failures.push(format!(
                "{names:?}: rust claims {rust:?} from empty markers"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} layouts disagree with the marker table:\n{}",
        failures.len(),
        layouts.len(),
        failures[..failures.len().min(12)].join("\n")
    );
    eprintln!(
        "{} layouts over {n} markers agree with the marker table; rust claims none of them",
        layouts.len()
    );
}

fn unsupported_name(path: &str) -> String {
    match path {
        ".cursor" | "AppData/Roaming/Cursor" => "Cursor",
        ".aider.chat.history.md" => "aider",
        ".local/share/crush/projects.json" | "AppData/Local/crush/projects.json" => "Crush",
        ".config/gitlab/duo" => "GitLab Duo CLI",
        other => panic!("no product name for {other}"),
    }
    .to_owned()
}

/// Every golden case placed at its default root, with no environment: the
/// TypeScript probe and the Rust reader pass each claim exactly that harness,
/// alone, with its Linux root relocated to each sibling root the detectors
/// probe for another operating system, and with every other case present in
/// the same home.
#[test]
fn rust_and_typescript_agree_on_golden_homes() {
    let cases = golden_cases();
    let scratch = Scratch::new("golden-homes");
    let all = scratch.0.join("all");
    fs::create_dir_all(&all).unwrap();
    let mut expected: Vec<(String, Vec<String>, Home)> = Vec::new();
    let mut relocated = vec![0; SIBLING_ROOTS.len()];
    for case in &cases {
        let home = scratch.0.join(case.id().replace('/', "-"));
        fs::create_dir_all(&home).unwrap();
        seed_default_root(case, &home);
        seed_default_root(case, &all);
        let want = vec![case.harness.clone()];
        for (i, (from, to)) in SIBLING_ROOTS.iter().enumerate() {
            if !home.join(from).exists() {
                continue;
            }
            let sibling = scratch
                .0
                .join(format!("{}-{i}", case.id().replace('/', "-")));
            relocate(&home, &sibling, from, to);
            expected.push((
                format!("{} at {to}", case.id()),
                want.clone(),
                Home::bare(&sibling),
            ));
            relocated[i] += 1;
        }
        expected.push((case.id(), want, Home::bare(&home)));
    }
    for (i, n) in relocated.iter().enumerate() {
        assert!(*n > 0, "no golden home holds {:?}", SIBLING_ROOTS[i]);
    }
    let every: Vec<String> = cases
        .iter()
        .map(|c| c.harness.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    expected.push(("all cases in one home".to_owned(), every, Home::bare(&all)));
    let ts = ts_detect(expected.iter().map(|(_, _, h)| h));
    let mut failures = Vec::new();
    for ((label, want, home), ts) in expected.iter().zip(&ts) {
        let rust = rust_detect(home);
        if ts.supported != *want || rust.supported != *want {
            failures.push(format!(
                "{label}: typescript {:?}, rust {:?}, want {want:?}",
                ts.supported, rust.supported
            ));
        }
        if !ts.unsupported.is_empty() {
            failures.push(format!("{label}: unsupported {:?}", ts.unsupported));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "{} golden homes, {} of them with a root relocated to a sibling, claim exactly their harness in both detectors",
        expected.len(),
        relocated.iter().sum::<usize>()
    );
}

/// A recordings root that is missing, or holds no `<harness>/<endpoint>/`
/// home, fails the replay instead of passing with an empty table.
#[test]
fn empty_recordings_root_fails_the_replay() {
    let scratch = Scratch::new("no-homes");
    fs::create_dir_all(scratch.0.join("harness-without-endpoints")).unwrap();
    for root in [scratch.0.join("missing"), scratch.0.clone()] {
        let message = catch_unwind(|| recorded_homes(&root))
            .err()
            .and_then(|p| p.downcast::<String>().ok())
            .unwrap_or_else(|| panic!("{} passed the replay", root.display()));
        assert_eq!(
            *message,
            format!("{} holds no <harness>/<endpoint>/ home", root.display())
        );
    }
}

/// Recorded homes, when `SKOPLI_VERIFY_HOMES` names a directory of
/// `<harness>/<endpoint>/` homes: both detectors run read-only on each, every
/// harness the Rust pass yields events for is also probed by TypeScript, and
/// the per-home table is written to `SKOPLI_VERIFY_HOMES_OUT` when set.
#[test]
fn recorded_homes_probe_every_harness_that_yields_events() {
    let Ok(root) = std::env::var("SKOPLI_VERIFY_HOMES") else {
        eprintln!("skipped: SKOPLI_VERIFY_HOMES is not set");
        return;
    };
    let homes = recorded_homes(root.as_ref());
    let ts = ts_detect(homes.iter().map(|(_, h)| h));
    let mut table = vec![
        "| home | typescript supported | rust supported | events | unsupported |".to_owned(),
        "| --- | --- | --- | --- | --- |".to_owned(),
    ];
    let mut failures = Vec::new();
    for ((label, home), ts) in homes.iter().zip(&ts) {
        let rust = rust_detect(home);
        let ctx = skopli_wire::parse::context_from_options(&serde_json::json!({
            "home": home.home, "env": {}, "cwd": home.cwd
        }));
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for h in &rust.supported {
            counts.insert(h.clone(), read(h, &ctx).unwrap().events.len());
            if !ts.supported.contains(h) {
                failures.push(format!(
                    "{label}: rust yields {h} events but typescript does not probe it"
                ));
            }
        }
        table.push(format!(
            "| {label} | {} | {} | {} | {} |",
            ts.supported.join(", "),
            rust.supported.join(", "),
            counts
                .iter()
                .map(|(h, n)| format!("{h}={n}"))
                .collect::<Vec<_>>()
                .join(", "),
            ts.unsupported.join(", ")
        ));
    }
    let text = table.join("\n");
    println!("{text}");
    if let Ok(out) = std::env::var("SKOPLI_VERIFY_HOMES_OUT") {
        fs::write(out, format!("{text}\n")).unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use verify_skopli_core::detect::*;
use verify_skopli_core::readers::{Scratch, golden_cases, read, sorted_dirs};

/// The TypeScript detector over every layout with zero, one or two markers
/// present, plus all present: the verdict depends only on the layout, each
/// single marker is claimed by exactly the harness it was read off, and a pair
/// detects exactly the union of its singles.
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
    for (idx, got) in layouts.iter().zip(&first) {
        let want = expected(idx);
        if *got != want {
            let names: Vec<&str> = idx.iter().map(|&i| MARKERS[i].path).collect();
            failures.push(format!("{names:?}: expected {want:?}, got {got:?}"));
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
        "{} layouts over {n} markers agree with the marker table",
        layouts.len()
    );
}

fn unsupported_name(path: &str) -> String {
    match path {
        ".cursor" => "Cursor",
        ".aider.chat.history.md" => "aider",
        ".local/share/crush/projects.json" => "Crush",
        ".config/gitlab/duo" => "GitLab Duo CLI",
        other => panic!("no product name for {other}"),
    }
    .to_owned()
}

/// Every golden case placed at its default root, with no environment: the
/// TypeScript probe and the Rust reader pass each claim exactly that harness,
/// alone and with every other case present in the same home.
#[test]
fn rust_and_typescript_agree_on_golden_homes() {
    let cases = golden_cases();
    let scratch = Scratch::new("golden-homes");
    let all = scratch.0.join("all");
    fs::create_dir_all(&all).unwrap();
    let mut homes = Vec::new();
    for case in &cases {
        let home = scratch.0.join(case.id().replace('/', "-"));
        fs::create_dir_all(&home).unwrap();
        seed_default_root(case, &home);
        seed_default_root(case, &all);
        homes.push(Home::bare(&home));
    }
    homes.push(Home::bare(&all));
    let ts = ts_detect(&homes);
    let mut failures = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        let rust = rust_detect(&homes[i]);
        let want = vec![case.harness.clone()];
        if ts[i].supported != want || rust.supported != want {
            failures.push(format!(
                "{}: typescript {:?}, rust {:?}",
                case.id(),
                ts[i].supported,
                rust.supported
            ));
        }
        if !ts[i].unsupported.is_empty() {
            failures.push(format!(
                "{}: unsupported {:?}",
                case.id(),
                ts[i].unsupported
            ));
        }
    }
    let every: Vec<String> = cases
        .iter()
        .map(|c| c.harness.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let rust_all = rust_detect(homes.last().unwrap());
    if ts.last().unwrap().supported != every || rust_all.supported != every {
        failures.push(format!(
            "all cases in one home: typescript {:?}, rust {:?}, want {every:?}",
            ts.last().unwrap().supported,
            rust_all.supported
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
    let mut homes = Vec::new();
    let mut labels = Vec::new();
    for harness in sorted_dirs(root.as_ref()) {
        for endpoint in sorted_dirs(&harness) {
            labels.push(format!(
                "{}/{}",
                harness.file_name().unwrap().to_string_lossy(),
                endpoint.file_name().unwrap().to_string_lossy()
            ));
            homes.push(Home::bare(&endpoint));
        }
    }
    let ts = ts_detect(&homes);
    let mut table = vec![
        "| home | typescript supported | rust supported | events | unsupported |".to_owned(),
        "| --- | --- | --- | --- | --- |".to_owned(),
    ];
    let mut failures = Vec::new();
    for ((label, home), ts) in labels.iter().zip(&homes).zip(&ts) {
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

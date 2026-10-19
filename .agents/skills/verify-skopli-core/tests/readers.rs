use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde_json::Value;
use skopli_core::readers::shared::date_parse_ms;
use skopli_core::time::to_iso_string;
use verify_skopli_core::readers::*;

/// Group failures by class and show the first counterexamples of each.
fn report(kind: &str, failures: Vec<(String, String)>, total: usize) {
    let mut by_class: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (class, detail) in &failures {
        by_class
            .entry(class.clone())
            .or_default()
            .push(detail.clone());
    }
    assert!(
        by_class.is_empty(),
        "{kind}: {} of {total} runs failed:\n{}",
        failures.len(),
        by_class
            .iter()
            .map(|(class, d)| format!(
                "- {class} ({} runs):\n    {}",
                d.len(),
                d[..d.len().min(8)].join("\n    ")
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Every reader survives every single-cell replacement and every byte-level
/// cut of its golden input. An event whose timestamp no longer parses may
/// leave the reader, but the wire read the facades call must then drop it
/// with a diagnostic instead of keeping it.
#[test]
fn readers_never_panic_on_damaged_input() {
    let mut failures = Vec::new();
    let mut runs = 0;
    let mut unusable = 0;
    for case in golden_cases() {
        let scratch = Scratch::new("total");
        let Seed { input, ctx, opts } = seed(&case, &scratch.0);
        for rel in files_under(&input) {
            let path = input.join(&rel);
            let original = fs::read(&path).unwrap();
            let text = String::from_utf8(original.clone()).unwrap_or_default();
            let mut variants: Vec<(String, Vec<u8>)> = damaged(&original)
                .into_iter()
                .map(|(label, bytes)| (label.to_owned(), bytes))
                .collect();
            for doc in docs(&path, &text) {
                for leaf in leaves(&doc.value) {
                    for (label, replacement) in adversarial_values() {
                        let mut mutated = Doc {
                            line: doc.line,
                            value: doc.value.clone(),
                        };
                        match replacement {
                            Some(v) => *get_mut(&mut mutated.value, &leaf) = v,
                            None if leaf.is_empty() => continue,
                            None => remove(&mut mutated.value, &leaf),
                        }
                        write_doc(&path, &text, &mutated);
                        variants.push((
                            format!("{} = {label}", leaf.join(".")),
                            fs::read(&path).unwrap(),
                        ));
                    }
                }
            }
            for (label, bytes) in variants {
                fs::write(&path, &bytes).unwrap();
                runs += 1;
                let where_ = format!("{} {} [{label}]", case.id(), rel.display());
                let result = match read(&case.harness, &ctx) {
                    Err(panic) => {
                        failures.push((
                            format!("panic in {}", case.id()),
                            format!("{where_}: {panic}"),
                        ));
                        continue;
                    }
                    Ok(r) => r,
                };
                let bad = result
                    .events
                    .iter()
                    .filter(|e| date_parse_ms(&e.timestamp).is_none())
                    .count();
                if bad > 0 {
                    unusable += 1;
                    let (kept, diagnostics) = wire_read(&opts);
                    let dropped = diagnostics
                        .iter()
                        .filter(|d| d.contains("unusable timestamp"))
                        .count();
                    if kept != result.events.len() - bad || dropped != bad {
                        failures.push((
                            format!("unusable timestamp kept by the wire for {}", case.harness),
                            format!("{where_}: {bad} unusable, wire kept {kept} of {} and dropped {dropped}", result.events.len()),
                        ));
                    }
                }
            }
            fs::write(&path, &original).unwrap();
        }
    }
    eprintln!(
        "{runs} damaged inputs, {unusable} of them left the reader with an unusable timestamp"
    );
    report("totality", failures, runs);
}

/// Silent drops the check finds in the product today. Each entry pins a
/// reader, a cell path suffix and the mutation; the check fails when one of
/// them stops reproducing so the list cannot outlive its fix.
const KNOWN_SILENT_DROPS: &[(&str, &str, &str)] = &[
    ("amp", "usageLedger.events.1.tokens.input", "text"),
    ("amp", "usageLedger.events.1.tokens.output", "text"),
    ("grok", "ctx.prompt_tokens", "text"),
    ("grok", "ctx.completion_tokens", "text"),
];

/// Token cells a reader reads around on purpose, each with the reason the
/// reader's own documentation gives. A cell matches when its harness is the
/// first field and `path=value` contains the second. An entry no mutation
/// touches fails the check so the list cannot drift from the corpus.
const IGNORED_BY_DESIGN: &[(&str, &str, &str)] = &[
    (
        "amp",
        "messages.",
        "per-message usage is the fallback for a thread without a usage ledger; the golden thread has one",
    ),
    (
        "claude",
        "cache_creation.ephemeral_5m_input_tokens",
        "the 5m write is cache_creation_input_tokens minus the 1h write, both of which are read",
    ),
    (
        "claude",
        "cache_creation.ephemeral_1h_input_tokens=-",
        "a negative 1h split is dropped as absent, so the record stays at zero after the bump",
    ),
    (
        "deepseek",
        "data.chunk.usage.",
        "the early chunk sample is superseded by the message usage of the same step",
    ),
    (
        "fx",
        "snapshot.input_tokens",
        "snapshot totals restate the per-model rows, which are read",
    ),
    (
        "fx",
        "snapshot.output_tokens",
        "snapshot totals restate the per-model rows, which are read",
    ),
    (
        "fx",
        "snapshot.cache_read_tokens",
        "snapshot totals restate the per-model rows, which are read",
    ),
    (
        "fx",
        "snapshot.cache_write_tokens",
        "snapshot totals restate the per-model rows, which are read",
    ),
    (
        "fx",
        "snapshot.reasoning_tokens",
        "snapshot totals restate the per-model rows, which are read",
    ),
    (
        "prime",
        "child_usage_attributed.",
        "usage attributed from child sessions is not the message's own usage",
    ),
    (
        "reasonix",
        "status.cumulative.promptTokens",
        "the prompt total is split into cacheHitTokens and cacheMissTokens, which are read",
    ),
];

/// Readers whose golden input holds no token cell the mapping check can
/// reach: SQLite databases, or counters inside a JSON string. The check fails
/// when this list and the set of readers with no live mutation differ.
const NO_MAPPING_COVERAGE: &[&str] = &[
    "cherrystudio",
    "cline",
    "goose",
    "hermes",
    "kilo",
    "kilocode",
    "kiro",
    "roo",
    "zed",
];

/// Bumping one live token cell by one moves exactly one of its own counters
/// by one, at most its derived counter by minus one, or turns the record into
/// a diagnostic; overwriting it with text never loses the record silently. A
/// cell is live when removing its record changes the events.
#[test]
fn token_cells_reach_their_own_event_field() {
    let mut failures = Vec::new();
    let mut runs = 0;
    let mut inert = 0;
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut reproduced: BTreeSet<(&str, &str, &str)> = BTreeSet::new();
    let mut designed: BTreeSet<(&str, &str, &str)> = BTreeSet::new();
    let cases = golden_cases();
    for case in &cases {
        let scratch = Scratch::new("map");
        let Seed { input, ctx, .. } = seed(case, &scratch.0);
        let baseline = read(&case.harness, &ctx).unwrap();
        let base = Outcome::of(&baseline);
        for rel in files_under(&input) {
            let path = input.join(&rel);
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let mut live: BTreeMap<Vec<String>, bool> = BTreeMap::new();
            for doc in docs(&path, &text) {
                for leaf in leaves(&doc.value) {
                    let Some(class) = TokenClass::of(&leaf) else {
                        continue;
                    };
                    let Some(n) = integer_cell(get(&doc.value, &leaf)) else {
                        continue;
                    };
                    let record = record_of(&leaf).to_vec();
                    let key: Vec<String> = doc
                        .line
                        .iter()
                        .map(|l| l.to_string())
                        .chain(record.iter().cloned())
                        .collect();
                    let is_live = *live.entry(key).or_insert_with(|| {
                        if record.is_empty() {
                            delete_doc(&path, &text, &doc);
                        } else {
                            let mut without = Doc {
                                line: doc.line,
                                value: doc.value.clone(),
                            };
                            remove(&mut without.value, &record);
                            write_doc(&path, &text, &without);
                        }
                        let r = read(&case.harness, &ctx).unwrap();
                        fs::write(&path, &text).unwrap();
                        r.events != baseline.events
                    });
                    if !is_live {
                        inert += 1;
                        continue;
                    }
                    let cell = leaf.join(".");
                    if let Some(entry) = IGNORED_BY_DESIGN
                        .iter()
                        .find(|(h, c, _)| *h == case.harness && format!("{cell}={n}").contains(c))
                    {
                        designed.insert(*entry);
                        continue;
                    }
                    covered.insert(case.harness.clone());
                    for label in ["+1", "text"] {
                        let mut mutated = Doc {
                            line: doc.line,
                            value: doc.value.clone(),
                        };
                        if label == "+1" {
                            bump(&mut mutated.value, &leaf, class);
                        } else {
                            *get_mut(&mut mutated.value, &leaf) = Value::from("x");
                        }
                        write_doc(&path, &text, &mutated);
                        runs += 1;
                        let outcome = Outcome::of(&read(&case.harness, &ctx).unwrap());
                        let verdict = if label == "+1" {
                            judge_bump(class, &base, &outcome)
                        } else {
                            judge_corruption(&base, &outcome)
                        };
                        if let Err((kind, detail)) = verdict {
                            if let Some(known) = KNOWN_SILENT_DROPS.iter().find(|(h, c, l)| {
                                *h == case.harness && cell.ends_with(c) && *l == label
                            }) {
                                reproduced.insert(*known);
                                continue;
                            }
                            failures.push((
                                format!("{kind} in {}", case.id()),
                                format!(
                                    "{} {}:{} {cell} {label}: {detail}",
                                    case.id(),
                                    rel.display(),
                                    doc.line.map_or(0, |l| l + 1)
                                ),
                            ));
                        }
                    }
                }
            }
            fs::write(&path, &text).unwrap();
        }
    }
    let uncovered: Vec<&str> = cases
        .iter()
        .map(|c| c.harness.as_str())
        .filter(|h| !covered.contains(*h))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if uncovered != NO_MAPPING_COVERAGE {
        failures.push((
            "mapping coverage differs from NO_MAPPING_COVERAGE".to_owned(),
            format!("readers with no live mutation: {uncovered:?}"),
        ));
    }
    for known in KNOWN_SILENT_DROPS {
        if !reproduced.contains(known) {
            failures.push((
                "known finding no longer reproduces".to_owned(),
                format!("{known:?}"),
            ));
        }
    }
    for entry in IGNORED_BY_DESIGN {
        if !designed.contains(entry) {
            failures.push((
                "ignored-by-design entry matched no cell".to_owned(),
                format!("{entry:?}"),
            ));
        }
    }
    eprintln!(
        "{runs} live token cell mutations over {} readers, {inert} inert cells skipped, {} ignored-by-design entries exercised, {} known silent drops reproduced",
        covered.len(),
        designed.len(),
        reproduced.len()
    );
    report("mapping", failures, runs);
}

/// A diagnostic excuses exactly one vanished event and nothing else: a bump
/// that takes a neighbour with the rejected record, or alters one, is still a
/// cross-event attribution.
#[test]
fn a_diagnostic_excuses_one_vanished_event_only() {
    let event = |n: u8, input: i128| {
        let id = format!("e{n}");
        (
            (id.clone(), id.clone(), id.clone(), id),
            [input, 0, 0, 0, 0, 0],
        )
    };
    let outcome = |events: &[(Identity, [i128; 6])], diagnostics| Outcome {
        events: events.to_vec(),
        diagnostics,
    };
    let base = outcome(&[event(1, 1), event(2, 1), event(3, 1)], 0);
    let kind = |bumped: &Outcome| judge_bump(TokenClass::Input, &base, bumped).map_err(|f| f.0);
    assert_eq!(kind(&outcome(&[event(1, 1), event(3, 1)], 1)), Ok(()));
    assert_eq!(
        kind(&outcome(&[event(1, 1), event(3, 1)], 0)),
        Err("silent drop")
    );
    assert_eq!(
        kind(&outcome(&[event(1, 1)], 1)),
        Err("cross-event attribution")
    );
    assert_eq!(
        kind(&outcome(&[event(1, 1), event(3, 2)], 1)),
        Err("cross-event attribution")
    );
}

/// Two reads of the same home are identical, the event count is the golden
/// one, and every emitted timestamp is a real instant that survives a parse
/// and format round trip.
#[test]
fn readers_are_deterministic_and_timestamps_round_trip() {
    for case in golden_cases() {
        let scratch = Scratch::new("det");
        let Seed { ctx, .. } = seed(&case, &scratch.0);
        let first = read(&case.harness, &ctx).unwrap();
        let second = read(&case.harness, &ctx).unwrap();
        assert_eq!(
            first.events,
            second.events,
            "{}: events differ between reads",
            case.id()
        );
        assert_eq!(
            first.skipped,
            second.skipped,
            "{}: skipped differ",
            case.id()
        );
        assert_eq!(
            first.warnings,
            second.warnings,
            "{}: warnings differ",
            case.id()
        );
        assert_eq!(
            first.events.len(),
            case.expected_events().len(),
            "{}: event count differs from the golden expectation",
            case.id()
        );
        for e in &first.events {
            let ms = date_parse_ms(&e.timestamp).unwrap_or_else(|| {
                panic!("{}: unparseable timestamp {:?}", case.id(), e.timestamp)
            });
            let iso = to_iso_string(ms).unwrap();
            assert_eq!(
                date_parse_ms(&iso),
                Some(ms),
                "{}: {iso} does not round trip",
                case.id()
            );
            assert!(
                e.cost_usd.is_none_or(f64::is_finite),
                "{}: non-finite cost",
                case.id()
            );
        }
    }
}

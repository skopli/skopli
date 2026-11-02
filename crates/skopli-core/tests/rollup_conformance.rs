//! Day-bucketing and billing-block conformance, driven by the shared fixtures in
//! `golden/rollup-tz/` and `golden/rollup-block/`. Each case synthesizes one
//! usage event per timestamp, rolls them up in the case's zone and asserts the
//! resulting buckets (key + event count) equal the gold the language facades
//! replay over the C ABI.

use serde::Deserialize;
use skopli_core::rollup::{RollupBy, RollupOptions, rollup};
use skopli_core::types::{TokenCounts, UsageEvent};

#[allow(dead_code)]
mod support;

#[derive(Deserialize)]
struct Bucket {
    key: String,
    events: u64,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    tz: String,
    #[serde(rename = "blockMs")]
    block_ms: Option<i64>,
    timestamps: Vec<String>,
    expected: Vec<Bucket>,
}

fn event(ts: &str) -> UsageEvent {
    UsageEvent {
        harness: "h".to_owned(),
        timestamp: ts.to_owned(),
        session_id: "s".to_owned(),
        message_id: "m".to_owned(),
        turn: false,
        subagent: false,
        model: "m".to_owned(),
        tokens: TokenCounts::default(),
        calls: None,
        cost_usd: None,
        workspace: None,
        title: None,
    }
}

fn replay(fixture: &str, by: RollupBy) {
    let path = support::repo_root()
        .join("golden")
        .join(fixture)
        .join("cases.json");
    let cases: Vec<Case> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read cases.json"))
            .expect("parse cases.json");
    assert!(!cases.is_empty(), "no {fixture} cases found");
    for case in &cases {
        let events: Vec<UsageEvent> = case.timestamps.iter().map(|t| event(t)).collect();
        let options = RollupOptions {
            by,
            tz: Some(case.tz.clone()),
            block_ms: case.block_ms,
        };
        let out = rollup(&events, &options);
        assert_eq!(
            out.len(),
            case.expected.len(),
            "{}: bucket count",
            case.name
        );
        for (got, want) in out.iter().zip(case.expected.iter()) {
            assert_eq!(got.key, want.key, "{}: bucket key", case.name);
            assert_eq!(got.events, want.events, "{}: bucket events", case.name);
        }
    }
}

#[test]
fn rollup_tz_conformance() {
    replay("rollup-tz", RollupBy::Day);
}

#[test]
fn rollup_block_conformance() {
    replay("rollup-block", RollupBy::Block);
}

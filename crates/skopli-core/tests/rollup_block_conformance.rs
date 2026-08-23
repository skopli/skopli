//! Billing-block windowing conformance, driven by the SHARED fixtures in
//! golden/rollup-block/. Each case synthesizes one usage event per timestamp,
//! rolls them up into blocks of the case's width in the case's zone, and asserts
//! the resulting blocks (key + event count) equal the shared gold. The language
//! facades consume the same fixture over the C ABI, so a facade that anchors,
//! splits, or floors a block wrong fails against the same gold.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use skopli_core::rollup::{RollupBy, RollupOptions, rollup};
use skopli_core::types::{TokenCounts, UsageEvent};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

#[derive(Deserialize)]
struct Block {
    key: String,
    events: u64,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    tz: String,
    #[serde(rename = "blockMs")]
    block_ms: i64,
    timestamps: Vec<String>,
    expected: Vec<Block>,
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

#[test]
fn rollup_block_conformance() {
    let path = repo_root()
        .join("golden")
        .join("rollup-block")
        .join("cases.json");
    let cases: Vec<Case> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read cases.json"))
            .expect("parse cases.json");
    assert!(!cases.is_empty(), "no rollup-block cases found");

    for case in &cases {
        let events: Vec<UsageEvent> = case.timestamps.iter().map(|t| event(t)).collect();
        let out = rollup(
            &events,
            &RollupOptions {
                by: RollupBy::Block,
                tz: Some(case.tz.clone()),
                block_ms: Some(case.block_ms),
            },
        );
        assert_eq!(out.len(), case.expected.len(), "{}: block count", case.name);
        for (got, want) in out.iter().zip(case.expected.iter()) {
            assert_eq!(got.key, want.key, "{}: block key", case.name);
            assert_eq!(got.events, want.events, "{}: block events", case.name);
        }
    }
}

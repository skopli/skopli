//! Replays `golden/pricing/timestamps/cases.json` through `date_parse_ms`, the
//! one grammar behind cache stamps and rollup block boundaries. The TypeScript
//! facade replays the same file in `test/timestamps.test.ts`, so a parser rule
//! pinned for one twin is checked on the other.

#[allow(dead_code)]
mod support;

use serde_json::Value;
use skopli_core::time::{date_parse_ms, to_iso_string};

fn cases() -> Value {
    let path = support::repo_root().join("golden/pricing/timestamps/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("read timestamps/cases.json"))
        .expect("parse timestamps/cases.json")
}

#[test]
fn read_cases_parse_to_the_shared_epoch() {
    for case in cases()["read"].as_array().expect("read array") {
        let stamp = case["stamp"].as_str().expect("stamp");
        assert_eq!(
            date_parse_ms(stamp),
            case["epochMs"].as_i64(),
            "{}: {stamp}",
            case["name"]
        );
    }
}

#[test]
fn write_cases_round_trip_through_the_canonical_stamp() {
    for ms in cases()["write"].as_array().expect("write array") {
        let ms = ms.as_i64().expect("epoch ms");
        let stamp = to_iso_string(ms).expect("in range");
        assert_eq!(stamp.len(), 24, "{stamp}");
        assert!(stamp.ends_with('Z'), "{stamp}");
        assert_eq!(date_parse_ms(&stamp), Some(ms));
    }
}

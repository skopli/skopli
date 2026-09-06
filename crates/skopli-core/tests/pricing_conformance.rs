//! Pricing conformance: reproduce the hermetic priced-rollup gold with ZERO
//! network. Mirrors `runPricingCases` in scripts/export-golden.ts and the
//! synthetic events in test/golden-cases/pricing.ts.
//!
//! The committed snapshot payloads (`golden/pricing/catalogs/{openrouter,
//! litellm}.json`) are fed through the SAME parsers a facade would call after
//! fetching (the data-level catalog hook), so the parse -> match -> cost path
//! runs exactly as the TS exporter's file-backed FetchLike load path did, with
//! no HTTP anywhere. `fetchedAt` is pinned to the exporter's sentinel
//! (`2026-08-01T00:00:00.000Z`) since the TS side stamps it from the wall clock
//! and the gold pins it for reproducibility.
//!
//! costUsd is compared as EXACT f64 (the comparator's number rule), the
//! bit-exactness gate; a 1-ULP fallback is reserved but is only enabled if
//! bit-exact is disproved.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use skopli_core::pricing::parse::{parse_litellm, parse_openrouter};
use skopli_core::pricing::types::{ModelPrice, PricingCatalog, TierMode};
use skopli_core::pricing::{Pricing, PricingMode, RollupPricing};
use skopli_core::rollup::{RollupBy, RollupOptions, rollup};
use skopli_core::types::{TokenCounts, UsageEvent};

mod support;
use support::structural_eq;

/// The exporter pins `fetchedAt` to this sentinel in the priced gold.
const PINNED_FETCHED_AT: &str = "2026-08-01T00:00:00.000Z";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read snapshot"))
        .expect("parse snapshot")
}

fn tk(
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cache_write1h: Option<u64>,
    reasoning: u64,
) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_read,
        cache_write,
        cache_write1h,
        reasoning,
    }
}

fn event(message_id: &str, ts_sec: u32, model: &str, tokens: TokenCounts) -> UsageEvent {
    UsageEvent {
        harness: "opencode".to_owned(),
        timestamp: format!("2026-08-01T00:0{ts_sec}:00.000Z"),
        session_id: "s".to_owned(),
        message_id: message_id.to_owned(),
        turn: true,
        subagent: false,
        model: model.to_owned(),
        tokens,
        calls: None,
        cost_usd: None,
        workspace: None,
        title: None,
    }
}

/// The exact synthetic events from test/golden-cases/pricing.ts, `basic` case.
fn basic_events() -> Vec<UsageEvent> {
    vec![
        event("flat", 0, "gpt-5", tk(1000, 500, 0, 0, None, 0)),
        event(
            "tiered",
            1,
            "claude-sonnet-4-5",
            tk(200_000, 10_000, 20_000, 30_000, Some(10_000), 2_000),
        ),
        event(
            "base-1h",
            2,
            "claude-sonnet-4-5",
            tk(5_000, 1_000, 2_000, 4_000, Some(1_500), 0),
        ),
        event(
            "alias",
            3,
            "us.anthropic.claude-opus-4-6-20260115-v1:0",
            tk(800, 200, 0, 0, None, 0),
        ),
        event(
            "miss",
            4,
            "totally-unknown-model-9000",
            tk(100, 100, 0, 0, None, 0),
        ),
    ]
}

/// The canonical event sort the exporter applies before rolling up.
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

/// Serialize a `ModelPrice` to its wire shape, re-adding `tierMode` (the core
/// derive skips it). Mirrors `wire::model_price_json`.
fn model_price_json(price: &ModelPrice) -> Value {
    let mut value = serde_json::to_value(price).expect("serialize ModelPrice");
    if let Some(mode) = price.tier_mode
        && let Some(obj) = value.as_object_mut()
    {
        let name = match mode {
            TierMode::Marginal => "marginal",
            TierMode::WholeRequest => "whole-request",
        };
        obj.insert("tierMode".to_owned(), json!(name));
    }
    value
}

/// Serialize a priced rollup to the gold-file `pricing` shape (mirrors
/// `canonicalPricedRollup` in scripts/export-golden.ts), pinning `fetchedAt`.
fn canonical_priced_pricing(pricing: &RollupPricing) -> Value {
    match pricing {
        RollupPricing::Hit {
            hit,
            usd,
            tiered_aggregate,
        } => {
            let mut obj = serde_json::Map::new();
            obj.insert("priced".to_owned(), json!(true));
            obj.insert("model".to_owned(), json!(hit.model));
            obj.insert("key".to_owned(), json!(hit.key));
            obj.insert("price".to_owned(), model_price_json(&hit.price));
            obj.insert("source".to_owned(), json!(hit.source));
            obj.insert(
                "fetchedAt".to_owned(),
                match &hit.fetched_at {
                    Some(_) => json!(PINNED_FETCHED_AT),
                    None => Value::Null,
                },
            );
            obj.insert("usd".to_owned(), json!(usd));
            if *tiered_aggregate {
                obj.insert("tieredAggregate".to_owned(), json!(true));
            }
            Value::Object(obj)
        }
        RollupPricing::Miss { miss, usd } => {
            let mut obj = serde_json::Map::new();
            obj.insert("priced".to_owned(), json!(false));
            obj.insert("model".to_owned(), json!(miss.model));
            obj.insert("attempted".to_owned(), json!(miss.attempted));
            if let Some(reason) = &miss.reason {
                obj.insert("reason".to_owned(), json!(reason));
                obj.insert("key".to_owned(), json!(miss.key));
            }
            if let Some(usd) = usd {
                obj.insert("usd".to_owned(), json!(usd));
            }
            Value::Object(obj)
        }
    }
}

/// Serialize a full priced rollup to the gold-file rollup shape.
fn canonical_priced_rollup(pr: &skopli_core::pricing::PricedRollup) -> Value {
    // Start from the plain rollup serialization (key, tokens, events, turns,
    // calls, optional costUsd), then attach `pricing`.
    let mut base = serde_json::to_value(&pr.rollup).expect("serialize rollup");
    base.as_object_mut()
        .expect("rollup object")
        .insert("pricing".to_owned(), canonical_priced_pricing(&pr.pricing));
    base
}

#[test]
fn pricing_basic_priced_rollup() {
    let catalogs_dir = repo_root().join("golden").join("pricing").join("catalogs");
    let openrouter = parse_openrouter(&read_json(&catalogs_dir.join("openrouter.json")));
    let litellm = parse_litellm(&read_json(&catalogs_dir.join("litellm.json")));

    // Source priority mirrors the exporter: OpenRouter first, then LiteLLM.
    // The TS createPricing prepends an (empty) override catalog; an empty
    // catalog is skipped by lookup, so it is omitted here. fetchedAt on each
    // parsed catalog is a non-null sentinel (the cache stamps it in TS); pin it.
    let catalogs = vec![
        PricingCatalog {
            source: "openrouter".to_owned(),
            fetched_at: Some(PINNED_FETCHED_AT.to_owned()),
            prices: openrouter,
        },
        PricingCatalog {
            source: "litellm".to_owned(),
            fetched_at: Some(PINNED_FETCHED_AT.to_owned()),
            prices: litellm,
        },
    ];
    let pricing = Pricing::new(catalogs, None, PricingMode::Calculate);

    let events = sort_events(&basic_events());
    let rollups = rollup(&events, &RollupOptions::new(RollupBy::Model));
    let priced = pricing.price_rollups(&rollups);

    let actual = json!({
        "schema_version": 1,
        "by": "model",
        "rollups": priced.iter().map(canonical_priced_rollup).collect::<Vec<_>>(),
    });

    let expected = read_json(
        &repo_root()
            .join("golden")
            .join("pricing")
            .join("basic")
            .join("expected-priced-rollup.json"),
    );

    structural_eq(&actual, &expected, "").expect("pricing conformance mismatch");
}

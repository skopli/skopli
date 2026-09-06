//! The rollup + event-group / rollup pricing that is identical across the
//! direct-core bindings. `read_usage` envelope construction, harness detection,
//! and `create_pricing` option parsing stay in each binding crate because their
//! shapes differ; these three functions do not.
//!
//! All three delegate to the gold-tested typed core engine
//! (`skopli_core::rollup::rollup`, `Pricing::price_events`,
//! `Pricing::price_rollups`) and only serialize the typed results to the wire
//! JSON shapes — the bit-exact `usd` summation lives in the core, not here.

use serde_json::Value;
use skopli_core::pricing::Pricing;
use skopli_core::rollup::{RollupBy, RollupOptions, rollup};
use skopli_core::types::UsageEvent;

use crate::wire;

/// Parse the shared `{ by, tz?, blockMs? }` rollup options into typed
/// [`RollupOptions`]. `blockMs` applies only to `by == "block"`.
fn rollup_options(opts: &Value) -> Result<RollupOptions, String> {
    let by_name = opts
        .get("by")
        .and_then(Value::as_str)
        .ok_or_else(|| "rollup options require a \"by\" dimension".to_owned())?;
    let by =
        RollupBy::parse(by_name).ok_or_else(|| format!("unknown rollup dimension: {by_name}"))?;
    let tz = opts.get("tz").and_then(Value::as_str).map(str::to_owned);
    let block_ms = opts.get("blockMs").and_then(Value::as_i64);
    Ok(RollupOptions { by, tz, block_ms })
}

/// Roll up a batch of events by the requested dimension into the `Rollup[]` wire
/// shape.
pub fn rollup_events(events: &[UsageEvent], opts: &Value) -> Result<Value, String> {
    let options = rollup_options(opts)?;
    let rollups = rollup(events, &options);
    let json: Vec<Value> = rollups
        .iter()
        .map(|r| serde_json::to_value(r).expect("serialize rollup"))
        .collect();
    Ok(Value::Array(json))
}

/// Price a batch of events grouped by the rollup dimension, producing the
/// `PricedEventGroup[]` wire shape. Delegates all pricing math to the typed
/// [`Pricing::price_events`] and only serializes the result.
pub fn price_events(
    pricing: &Pricing,
    events: &[UsageEvent],
    opts: &Value,
) -> Result<Value, String> {
    let options = rollup_options(opts)?;
    let groups = pricing.price_events(events, &options);

    let mut out: Vec<Value> = Vec::with_capacity(groups.len());
    for group in &groups {
        let mut base = serde_json::to_value(&group.rollup).expect("serialize rollup");
        let obj = base.as_object_mut().expect("rollup object");
        obj.insert(
            "pricing".to_owned(),
            wire::group_pricing_json(&group.pricing),
        );
        out.push(base);
    }
    Ok(Value::Array(out))
}

/// Price a set of rollups, producing the `PricedRollup[]` wire shape with the
/// `pricing` field attached. Delegates the match + cost math to the typed
/// [`Pricing::price_rollups`]; the incoming JSON is parsed into typed
/// [`Rollup`](skopli_core::rollup::Rollup)s first so the shipping path and
/// the gold-tested core path are the SAME code.
pub fn price_rollups(pricing: &Pricing, rollups_json: &Value) -> Result<Value, String> {
    use skopli_core::pricing::RollupPricing;

    let arr = rollups_json
        .as_array()
        .ok_or_else(|| "rollups must be a JSON array".to_owned())?;

    let mut rollups = Vec::with_capacity(arr.len());
    for entry in arr {
        rollups.push(crate::parse::parse_rollup(entry)?);
    }

    let priced = pricing.price_rollups(&rollups);

    let mut out: Vec<Value> = Vec::with_capacity(priced.len());
    for (entry, pr) in arr.iter().zip(priced.iter()) {
        // Preserve the caller's original rollup object verbatim (so any extra
        // keys survive) and attach only the computed `pricing` field.
        let mut base = entry.clone();
        let base_obj = base.as_object_mut().expect("rollup object");
        let pricing_value = match &pr.pricing {
            RollupPricing::Hit {
                hit,
                usd,
                tiered_aggregate,
            } => wire::price_hit_json(hit, &hit.model, *usd, *tiered_aggregate),
            RollupPricing::Miss { miss, usd } => wire::price_miss_json(
                &miss.model,
                &miss.attempted,
                miss.reason.as_deref(),
                miss.key.as_deref(),
                *usd,
            ),
        };
        base_obj.insert("pricing".to_owned(), pricing_value);
        out.push(base);
    }
    Ok(Value::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event_json(ts: &str) -> Value {
        json!({
            "harness": "h",
            "timestamp": ts,
            "sessionId": "s",
            "messageId": "m",
            "turn": false,
            "subagent": false,
            "model": "m",
            "tokens": {
                "input": 1,
                "output": 2,
                "cacheRead": 3,
                "cacheWrite": 10,
                "reasoning": 4
            }
        })
    }

    fn keys(events: Value, opts: Value) -> Vec<String> {
        let parsed = crate::parse::parse_events(&events).expect("parse events");
        let out = rollup_events(&parsed, &opts).expect("rollup");
        out.as_array()
            .expect("array")
            .iter()
            .map(|r| r["key"].as_str().expect("key").to_owned())
            .collect()
    }

    #[test]
    fn block_extreme_width_never_splits_or_panics() {
        // A blockMs near i64::MAX saturates the block end so every later event
        // stays in the first block; no panic (debug) or wrap (release).
        let events = json!([
            event_json("2026-01-01T09:00:00.000Z"),
            event_json("2026-01-01T14:00:00.000Z"),
            event_json("2030-06-01T00:00:00.000Z"),
        ]);
        let opts = json!({ "by": "block", "tz": "UTC", "blockMs": i64::MAX });
        let out = keys(events, opts);
        assert_eq!(out, vec!["2026-01-01T09:00:00.000Z".to_owned()]);
    }

    #[test]
    fn block_omitted_width_applies_the_five_hour_default() {
        // Options parsed with no "blockMs" must fall back to the five-hour
        // default: two events 3h41m apart share one block, and an event past
        // five hours opens a new one. Guards the wire against a regression that
        // stops applying DEFAULT_BLOCK_MS when the optional width is absent.
        let joined = json!([
            event_json("2026-01-01T09:17:00.000Z"),
            event_json("2026-01-01T13:00:00.000Z"),
        ]);
        let opts = json!({ "by": "block", "tz": "UTC" });
        assert_eq!(
            keys(joined, opts),
            vec!["2026-01-01T09:00:00.000Z".to_owned()]
        );

        let split = json!([
            event_json("2026-01-01T09:00:00.000Z"),
            event_json("2026-01-01T14:30:00.000Z"),
        ]);
        let opts = json!({ "by": "block", "tz": "UTC" });
        assert_eq!(
            keys(split, opts),
            vec![
                "2026-01-01T09:00:00.000Z".to_owned(),
                "2026-01-01T14:00:00.000Z".to_owned(),
            ]
        );
    }
}

//! Wire-shape serializers for the pricing value types the core exposes with
//! non-`Serialize` idiomatic shapes (`PriceHit`, `PriceMiss`, `PriceLookup`,
//! `CatalogInfo`, `ModelPrice`). These mirror the TS JSON shapes in
//! pricing/index.ts and pricing/types.ts exactly (camelCase keys, absent vs null
//! discipline).

use serde_json::{Value, json};
use skopli_core::pricing::types::TierMode;
use skopli_core::pricing::{CatalogInfo, GroupPricing, ModelPrice, PriceHit};

/// Serialize a `ModelPrice` to its wire shape, re-adding `tierMode` (the core
/// derive skips it, but the facade's `ModelPrice` carries it).
pub fn model_price_json(price: &ModelPrice) -> Value {
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

/// The single-model priced-group `pricing` shape: a hit lookup plus `usd`, and
/// `tieredAggregate: true` when the group aggregated a tiered model across
/// multiple calls. `model` overrides the hit's model (the group's sole model).
pub fn price_hit_json(hit: &PriceHit, model: &str, usd: f64, tiered_aggregate: bool) -> Value {
    let mut obj = json!({
        "priced": true,
        "model": model,
        "key": hit.key,
        "price": model_price_json(&hit.price),
        "source": hit.source,
        "fetchedAt": match &hit.fetched_at {
            Some(v) => json!(v),
            None => Value::Null,
        },
        "usd": usd,
    });
    if tiered_aggregate {
        obj.as_object_mut()
            .unwrap()
            .insert("tieredAggregate".to_owned(), json!(true));
    }
    obj
}

/// The miss priced-group `pricing` shape: a miss lookup plus optional `usd`.
pub fn price_miss_json(
    model: &str,
    attempted: &[String],
    reason: Option<&str>,
    key: Option<&str>,
    usd: Option<f64>,
) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("priced".to_owned(), json!(false));
    obj.insert("model".to_owned(), json!(model));
    obj.insert("attempted".to_owned(), json!(attempted));
    if let Some(reason) = reason {
        obj.insert("reason".to_owned(), json!(reason));
        obj.insert("key".to_owned(), json!(key));
    }
    if let Some(usd) = usd {
        obj.insert("usd".to_owned(), json!(usd));
    }
    Value::Object(obj)
}

/// The `pricing` field of a priced event group ([`GroupPricing`]). Mirrors the
/// `priceEvents` per-group shape: single-model groups carry the full hit/miss
/// detail, multi-model groups carry only the aggregate `usd` (+ sorted models on
/// a hit).
pub fn group_pricing_json(pricing: &GroupPricing) -> Value {
    match pricing {
        GroupPricing::SingleHit {
            hit,
            model,
            usd,
            tiered_aggregate,
        } => price_hit_json(hit, model, *usd, *tiered_aggregate),
        GroupPricing::MultiHit { usd, models } => {
            json!({ "priced": true, "usd": usd, "models": models })
        }
        GroupPricing::SingleMiss { miss, usd } => price_miss_json(
            &miss.model,
            &miss.attempted,
            miss.reason.as_deref(),
            miss.key.as_deref(),
            *usd,
        ),
        GroupPricing::MultiMiss {
            key,
            attempted,
            usd,
        } => price_miss_json(key, attempted, None, None, *usd),
        GroupPricing::Empty { key } => {
            json!({ "priced": false, "model": key, "attempted": [key] })
        }
    }
}

/// The lookup result shape (`PriceHit | PriceMiss`) that `lookup_model` returns.
pub fn price_lookup_json(lookup: &skopli_core::pricing::PriceLookup) -> Value {
    use skopli_core::pricing::PriceLookup;
    match lookup {
        PriceLookup::Hit(hit) => json!({
            "priced": true,
            "model": hit.model,
            "key": hit.key,
            "price": model_price_json(&hit.price),
            "source": hit.source,
            "fetchedAt": match &hit.fetched_at {
                Some(v) => json!(v),
                None => Value::Null,
            },
        }),
        PriceLookup::Miss(miss) => price_miss_json(
            &miss.model,
            &miss.attempted,
            miss.reason.as_deref(),
            miss.key.as_deref(),
            None,
        ),
    }
}

/// Serialize a `CatalogInfo` to `{source, fetchedAt, models}`.
pub fn catalog_info_json(info: &CatalogInfo) -> Value {
    json!({
        "source": info.source,
        "fetchedAt": match &info.fetched_at {
            Some(v) => json!(v),
            None => Value::Null,
        },
        "models": info.models,
    })
}

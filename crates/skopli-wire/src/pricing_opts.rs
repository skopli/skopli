//! Build a core [`Pricing`] instance from the `create_pricing` options JSON,
//! shared by the direct-core bindings whose facade owns network fetching and the
//! on-disk cache (`skopli-py`, `skopli-ruby`). `skopli-capi` and
//! `skopli-node` carry their own option shapes (the capi one has an FFI
//! layer; node lacks the `overrides` path), so they do NOT use this module.
//!
//! Seam split: a caller supplies pricing either as programmatic
//! `overrides` or as pre-fetched `catalogs` (each a source name + optional
//! fetchedAt + a prices map already in the core catalog grammar, OR a raw source
//! payload plus its `format`). `builtin_sources`/`offline`/`max_cache_age_ms`
//! steer facade-side fetching, so they are accepted-and-ignored keys here (this
//! crate never fetches).

use serde_json::Value;
use skopli_core::pricing::parse::{parse_litellm, parse_models_dev, parse_openrouter};
use skopli_core::pricing::types::{PriceMap, PricingCatalog};
use skopli_core::pricing::{Pricing, PricingMode};

use crate::parse::parse_price;

/// Parse the `create_pricing` options into a [`Pricing`] instance, or an error
/// message (turned into a `CatalogError` by the facade).
pub fn pricing_from_options(opts: &Value) -> Result<Pricing, String> {
    let (catalogs, override_index, mode) = parse_pricing_parts(opts)?;
    Ok(Pricing::new(catalogs, override_index, mode))
}

/// Parse the shared `create_pricing` options into the raw parts of a [`Pricing`]
/// — the priority-ordered catalog list, the override catalog index (if any), and
/// the mode — WITHOUT constructing the instance. Bindings whose seam adds extra
/// catalogs before construction (the C ABI appends built-in sources at the
/// lowest priority) build on this; [`pricing_from_options`] is the plain path.
pub fn parse_pricing_parts(
    opts: &Value,
) -> Result<(Vec<PricingCatalog>, Option<usize>, PricingMode), String> {
    let mode = match opts.get("mode").and_then(Value::as_str) {
        Some(name) => {
            PricingMode::parse(name).ok_or_else(|| format!("unknown pricing mode: {name}"))?
        }
        None => PricingMode::Calculate,
    };

    let mut catalogs: Vec<PricingCatalog> = Vec::new();
    let mut override_index: Option<usize> = None;

    // Overrides become the highest-priority catalog (source "override", no
    // fetchedAt), mirroring the TS createPricing which prepends the override map.
    if let Some(overrides) = opts.get("overrides").and_then(Value::as_array)
        && !overrides.is_empty()
    {
        let mut prices = PriceMap::new();
        for entry in overrides {
            let obj = entry
                .as_object()
                .ok_or_else(|| "each override must be an object".to_owned())?;
            let model = obj
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| "each override needs a \"model\"".to_owned())?;
            prices.insert(model.to_owned(), parse_price(entry)?);
        }
        override_index = Some(catalogs.len());
        catalogs.push(PricingCatalog {
            source: "override".to_owned(),
            fetched_at: None,
            prices,
        });
    }

    // Pre-fetched catalogs, in the order given (priority order after overrides).
    if let Some(list) = opts.get("catalogs").and_then(Value::as_array) {
        for entry in list {
            catalogs.push(parse_catalog(entry)?);
        }
    }

    Ok((catalogs, override_index, mode))
}

/// Parse a single pre-fetched catalog entry. Two forms are accepted:
///   - `{source, fetchedAt?, prices: {model: ModelPrice}}` - already parsed;
///   - `{source, fetchedAt?, format: "openrouter"|"litellm"|"modelsdev",
///     payload: <raw source JSON>}` - parsed via the core parsers.
fn parse_catalog(entry: &Value) -> Result<PricingCatalog, String> {
    let obj = entry
        .as_object()
        .ok_or_else(|| "each catalog must be an object".to_owned())?;
    let source = obj
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| "each catalog needs a \"source\"".to_owned())?
        .to_owned();
    let fetched_at = obj
        .get("fetchedAt")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let prices = if let Some(format) = obj.get("format").and_then(Value::as_str) {
        let payload = obj
            .get("payload")
            .ok_or_else(|| "a catalog with a \"format\" needs a \"payload\"".to_owned())?;
        match format {
            "openrouter" => parse_openrouter(payload),
            "litellm" => parse_litellm(payload),
            "modelsdev" | "models.dev" => parse_models_dev(payload),
            other => return Err(format!("unknown catalog format: {other}")),
        }
    } else {
        let prices_obj = obj
            .get("prices")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                "a catalog needs either \"prices\" or a \"format\"+\"payload\"".to_owned()
            })?;
        let mut map = PriceMap::new();
        for (model, price_value) in prices_obj {
            map.insert(model.clone(), parse_price(price_value)?);
        }
        map
    };

    Ok(PricingCatalog {
        source,
        fetched_at,
        prices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use skopli_core::pricing::types::{ModelPrice, PriceLookup, TierMode};

    fn hit(pricing: &Pricing, model: &str) -> Option<ModelPrice> {
        match pricing.lookup_model(model) {
            PriceLookup::Hit(hit) => Some(hit.price),
            PriceLookup::Miss(_) => None,
        }
    }

    fn tiered_price() -> Value {
        json!({
            "input": 1.0,
            "output": 2.0,
            "tiers": [{ "threshold": 200000, "input": 2.0, "output": 4.0 }],
            "tierMode": "whole-request"
        })
    }

    #[test]
    fn catalog_prices_keep_tiers_and_tier_mode() {
        let opts = json!({
            "catalogs": [{ "source": "litellm", "prices": { "m": tiered_price() } }]
        });
        let pricing = pricing_from_options(&opts).expect("pricing builds");
        let price = hit(&pricing, "m").expect("hit");
        assert_eq!(price.tiers.as_ref().map(Vec::len), Some(1));
        assert_eq!(price.tier_mode, Some(TierMode::WholeRequest));
    }

    #[test]
    fn overrides_keep_tiers() {
        let mut entry = tiered_price();
        entry["model"] = json!("m");
        let opts = json!({ "overrides": [entry] });
        let pricing = pricing_from_options(&opts).expect("pricing builds");
        let price = hit(&pricing, "m").expect("hit");
        assert_eq!(price.tiers.as_ref().map(Vec::len), Some(1));
    }
}

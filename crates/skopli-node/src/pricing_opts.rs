//! Build a core [`Pricing`] instance from the addon's `Pricing` constructor
//! options JSON.
//!
//! The Node facade keeps source loading / fetch / cache
//! in TS (sources.ts) and hands the core the ALREADY priority-ordered catalog
//! list it loaded (the TS `loadCatalogs` result), plus the index of the catalog
//! whose prices are the programmatic-override map. The facade sends:
//!
//! ```jsonc
//! {
//!   "mode": "calculate" | "display" | "auto",
//!   // index into `catalogs` of the override catalog, or omitted when none.
//!   // The override catalog does NOT skip zero-cost placeholders (an explicit
//!   // $0 override is intentional); identity-by-index mirrors the TS
//!   // `catalog.prices !== overrides` check, so a lower-priority market source
//!   // merely NAMED "override" is not treated as one.
//!   "overrideIndex": 0,
//!   // the full priority-ordered catalog list (highest first), exactly as the
//!   // TS facade loaded it - including the override catalog at `overrideIndex`
//!   // and any empty-coverage catalogs (kept so `catalogs()` provenance agrees).
//!   "catalogs": [ { "source": "override", "fetchedAt": null,
//!                   "prices": { "model": { "input": .., "output": .., .. } } } ]
//! }
//! ```

use serde_json::Value;
use skopli_core::pricing::types::{ModelPrice, PriceMap, PriceTier, PricingCatalog, TierMode};
use skopli_core::pricing::{Pricing, PricingMode};

/// Parse the `Pricing` constructor options into a [`Pricing`] instance, or an
/// error message.
pub(crate) fn pricing_from_options(opts: &Value) -> Result<Pricing, String> {
    let mode = match opts.get("mode").and_then(Value::as_str) {
        Some(name) => {
            PricingMode::parse(name).ok_or_else(|| format!("unknown pricing mode: {name}"))?
        }
        None => PricingMode::Calculate,
    };

    let mut catalogs: Vec<PricingCatalog> = Vec::new();
    if let Some(list) = opts.get("catalogs").and_then(Value::as_array) {
        for entry in list {
            catalogs.push(parse_catalog(entry)?);
        }
    }

    let override_index = opts
        .get("overrideIndex")
        .and_then(Value::as_u64)
        .map(|i| i as usize)
        .filter(|&i| i < catalogs.len());

    Ok(Pricing::new(catalogs, override_index, mode))
}

/// Parse a single catalog entry:
/// `{source, fetchedAt?, prices: {model: ModelPrice}}`. Preserves the price-map
/// insertion order (the matcher relies on same-catalog key priority).
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

    let prices_obj = obj
        .get("prices")
        .and_then(Value::as_object)
        .ok_or_else(|| "a catalog needs a \"prices\" map".to_owned())?;
    let mut map = PriceMap::new();
    for (model, price_value) in prices_obj {
        map.insert(model.clone(), parse_model_price(price_value)?);
    }

    Ok(PricingCatalog {
        source,
        fetched_at,
        prices: map,
    })
}

/// Parse a `ModelPrice` from a wire object (the full catalog shape, including
/// optional `tiers` and `tierMode` so tier-bearing LiteLLM catalogs loaded in TS
/// round-trip into the core matcher unchanged).
fn parse_model_price(value: &Value) -> Result<ModelPrice, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "a price must be an object".to_owned())?;
    let num = |k: &str| obj.get(k).and_then(Value::as_f64);
    let input = num("input").ok_or_else(|| "a price needs a numeric \"input\"".to_owned())?;
    let output = num("output").ok_or_else(|| "a price needs a numeric \"output\"".to_owned())?;
    let mut price = ModelPrice::flat(input, output);
    price.cache_read = num("cacheRead");
    price.cache_write = num("cacheWrite");
    price.cache_write1h = num("cacheWrite1h");
    if let Some(tiers) = obj.get("tiers").and_then(Value::as_array) {
        let mut parsed = Vec::with_capacity(tiers.len());
        for tier in tiers {
            let t = tier
                .as_object()
                .ok_or_else(|| "each tier must be an object".to_owned())?;
            let tnum = |k: &str| t.get(k).and_then(Value::as_f64);
            parsed.push(PriceTier {
                threshold: tnum("threshold")
                    .ok_or_else(|| "tier needs numeric \"threshold\"".to_owned())?,
                input: tnum("input").ok_or_else(|| "tier needs numeric \"input\"".to_owned())?,
                output: tnum("output").ok_or_else(|| "tier needs numeric \"output\"".to_owned())?,
                cache_read: tnum("cacheRead"),
                cache_write: tnum("cacheWrite"),
                cache_write1h: tnum("cacheWrite1h"),
            });
        }
        if !parsed.is_empty() {
            price.tiers = Some(parsed);
        }
    }
    price.tier_mode = match obj.get("tierMode").and_then(Value::as_str) {
        Some("marginal") => Some(TierMode::Marginal),
        Some("whole-request") => Some(TierMode::WholeRequest),
        _ => None,
    };
    Ok(price)
}

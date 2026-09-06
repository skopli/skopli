//! Source-payload parsers - the data-level catalog hook. Faithful port of the
//! parser bodies in `src/pricing/sources.ts` (`parseOpenRouter`,
//! `parseLiteLlm`, `parseLiteLlmTiers`, `parseModelsDev`).
//!
//! The core deliberately does NOT fetch or cache (network stays in facades;
//! core takes catalog JSON in). A facade fetches the raw
//! source payload, then calls the matching parser here to obtain a
//! [`PriceMap`]; the conformance harness feeds the committed snapshot payloads
//! through the very same parsers with ZERO network.

use serde_json::Value;

use super::types::{ModelPrice, PriceMap, PriceTier, TierMode};

/// `finite(value)` from sources.ts: a JSON number that is finite, or a
/// non-empty numeric string, else `None`. Matches the TS `Number(value)` /
/// `Number.isFinite` semantics for the string case.
fn finite(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64().filter(|v| v.is_finite()),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                return None;
            }
            // TS `Number(s)` parses the whole trimmed string; `parse::<f64>`
            // over the trimmed input matches for the payloads we accept
            // (decimal / exponent forms). A non-numeric string -> None.
            s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
        }
        _ => None,
    }
}

/// `rate1h(raw)` from sources.ts: a per-token above-1hr rate scaled to
/// per-million, dropping negative or overflowing values so a nonsensical price
/// cannot bypass the `input * 2` fallback.
fn rate1h(raw: Option<f64>) -> Option<f64> {
    let raw = raw?;
    if raw < 0.0 {
        return None;
    }
    let scaled = raw * 1e6;
    if scaled.is_finite() {
        Some(scaled)
    } else {
        None
    }
}

fn as_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value.as_object()
}

/// Parse an OpenRouter `/models` payload. Faithful port of `parseOpenRouter`.
/// OpenRouter reports USD per token as decimal strings.
pub fn parse_openrouter(payload: &Value) -> PriceMap {
    let mut prices = PriceMap::new();
    let Some(obj) = as_object(payload) else {
        return prices;
    };
    let Some(data) = obj.get("data").and_then(Value::as_array) else {
        return prices;
    };
    for entry in data {
        let Some(entry) = as_object(entry) else {
            continue;
        };
        let id = entry.get("id");
        let pricing = entry.get("pricing");
        let (Some(Value::String(id)), Some(pricing)) = (id, pricing) else {
            continue;
        };
        let Some(pricing) = as_object(pricing) else {
            continue;
        };
        let input = finite(pricing.get("prompt"));
        let output = finite(pricing.get("completion"));
        let (Some(input), Some(output)) = (input, output) else {
            continue;
        };
        let mut price = ModelPrice::flat(input * 1e6, output * 1e6);
        if let Some(cache_read) = finite(pricing.get("input_cache_read")) {
            price.cache_read = Some(cache_read * 1e6);
        }
        if let Some(cache_write) = finite(pricing.get("input_cache_write")) {
            price.cache_write = Some(cache_write * 1e6);
        }
        prices.insert(id.clone(), price);
    }
    prices
}

const LITELLM_TIER_THRESHOLDS: &[u64] = &[128, 200, 256, 272, 512];

/// Faithful port of `parseLiteLlmTiers`.
fn parse_litellm_tiers(entry: &serde_json::Map<String, Value>) -> Vec<PriceTier> {
    let mut tiers: Vec<PriceTier> = Vec::new();
    for &k in LITELLM_TIER_THRESHOLDS {
        let suffix = format!("_above_{k}k_tokens");
        let input = finite(entry.get(&format!("input_cost_per_token{suffix}")));
        let output = finite(entry.get(&format!("output_cost_per_token{suffix}")));
        let cache_read = finite(entry.get(&format!("cache_read_input_token_cost{suffix}")));
        let cache_write = finite(entry.get(&format!("cache_creation_input_token_cost{suffix}")));
        let tier_write1h = rate1h(finite(entry.get(&format!(
            "cache_creation_input_token_cost_above_1hr{suffix}"
        ))));
        if input.is_none()
            && output.is_none()
            && cache_read.is_none()
            && cache_write.is_none()
            && tier_write1h.is_none()
        {
            continue;
        }
        let prev = tiers.last();
        // base input/output: inherit from the previous tier, else the flat rate
        // (defaulting to 0 when the flat rate is absent), scaled to per-million.
        let base_input = prev
            .map(|p| p.input)
            .unwrap_or_else(|| finite(entry.get("input_cost_per_token")).unwrap_or(0.0) * 1e6);
        let base_output = prev
            .map(|p| p.output)
            .unwrap_or_else(|| finite(entry.get("output_cost_per_token")).unwrap_or(0.0) * 1e6);
        let mut tier = PriceTier {
            threshold: (k * 1000) as f64,
            input: match input {
                Some(v) => v * 1e6,
                None => base_input,
            },
            output: match output {
                Some(v) => v * 1e6,
                None => base_output,
            },
            cache_read: None,
            cache_write: None,
            cache_write1h: None,
        };
        let prev_cache_read = prev.and_then(|p| p.cache_read);
        let prev_cache_write = prev.and_then(|p| p.cache_write);
        if let Some(v) = cache_read {
            tier.cache_read = Some(v * 1e6);
        } else if let Some(v) = prev_cache_read {
            tier.cache_read = Some(v);
        }
        if let Some(v) = cache_write {
            tier.cache_write = Some(v * 1e6);
        } else if let Some(v) = prev_cache_write {
            tier.cache_write = Some(v);
        }
        if let Some(v) = tier_write1h {
            tier.cache_write1h = Some(v);
        }
        tiers.push(tier);
    }
    tiers
}

/// Parse a LiteLLM `model_prices_and_context_window` payload. Faithful port of
/// `parseLiteLlm`. LiteLLM reports USD per token as numbers, keyed by model.
pub fn parse_litellm(payload: &Value) -> PriceMap {
    let mut prices = PriceMap::new();
    let Some(obj) = as_object(payload) else {
        return prices;
    };
    for (model, entry) in obj {
        if model == "sample_spec" {
            continue;
        }
        let Some(entry) = as_object(entry) else {
            continue;
        };
        let input = finite(entry.get("input_cost_per_token"));
        let output = finite(entry.get("output_cost_per_token"));
        let (Some(input), Some(output)) = (input, output) else {
            continue;
        };
        let mut price = ModelPrice::flat(input * 1e6, output * 1e6);
        if let Some(cache_read) = finite(entry.get("cache_read_input_token_cost")) {
            price.cache_read = Some(cache_read * 1e6);
        }
        if let Some(cache_write) = finite(entry.get("cache_creation_input_token_cost")) {
            price.cache_write = Some(cache_write * 1e6);
        }
        let base_write1h = rate1h(finite(
            entry.get("cache_creation_input_token_cost_above_1hr"),
        ));
        if let Some(v) = base_write1h {
            price.cache_write1h = Some(v);
        }
        let tiers = parse_litellm_tiers(entry);
        if !tiers.is_empty() {
            price.tiers = Some(tiers);
            if model.contains("claude") {
                price.tier_mode = Some(TierMode::WholeRequest);
            }
        }
        prices.insert(model.clone(), price);
    }
    prices
}

/// Parse a models.dev `api.json` payload. Faithful port of `parseModelsDev`.
/// models.dev reports USD per million tokens, nested provider -> models.
pub fn parse_models_dev(payload: &Value) -> PriceMap {
    let mut prices = PriceMap::new();
    let Some(obj) = as_object(payload) else {
        return prices;
    };
    for (provider, provider_entry) in obj {
        let Some(provider_entry) = as_object(provider_entry) else {
            continue;
        };
        let Some(models) = provider_entry.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model, model_entry) in models {
            let Some(model_entry) = as_object(model_entry) else {
                continue;
            };
            let Some(cost) = model_entry.get("cost").and_then(Value::as_object) else {
                continue;
            };
            let input = finite(cost.get("input"));
            let output = finite(cost.get("output"));
            let (Some(input), Some(output)) = (input, output) else {
                continue;
            };
            let mut price = ModelPrice::flat(input, output);
            if let Some(cache_read) = finite(cost.get("cache_read")) {
                price.cache_read = Some(cache_read);
            }
            if let Some(cache_write) = finite(cost.get("cache_write")) {
                price.cache_write = Some(cache_write);
            }
            // provider-qualified key first, then the bare id as a lower-priority
            // same-catalog alias (only when not already present).
            prices.insert(format!("{provider}/{model}"), price.clone());
            if !prices.contains_key(model) {
                prices.insert(model.clone(), price);
            }
        }
    }
    prices
}

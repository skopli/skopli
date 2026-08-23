//! Pricing data types. Faithful port of `src/pricing/types.ts`.
//!
//! All prices are USD per million tokens. `f64` throughout - a decimal upgrade
//! is a possible later follow-up, so parity is judged against the TS `f64`
//! outputs first.

use serde::Serialize;

/// A long-context price tier. Mirrors `PriceTier` in src/pricing/types.ts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceTier {
    pub threshold: f64,
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead", skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(rename = "cacheWrite", skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    /// Rate for 1h-TTL cache writes at this tier; when omitted, falls back to
    /// this tier's own `input * 2` (never inherited from a lower level).
    #[serde(rename = "cacheWrite1h", skip_serializing_if = "Option::is_none")]
    pub cache_write1h: Option<f64>,
}

/// The tiering mode. Mirrors the `"marginal" | "whole-request"` union.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierMode {
    Marginal,
    WholeRequest,
}

/// A model's price entry. Mirrors `ModelPrice` in src/pricing/types.ts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead", skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(rename = "cacheWrite", skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    /// Rate for 1h-TTL cache writes (LiteLLM
    /// `cache_creation_input_token_cost_above_1hr`).
    #[serde(rename = "cacheWrite1h", skip_serializing_if = "Option::is_none")]
    pub cache_write1h: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<PriceTier>>,
    #[serde(skip_serializing)]
    pub tier_mode: Option<TierMode>,
}

impl ModelPrice {
    /// A flat price with no cache rates, tiers, or tier mode.
    pub fn flat(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_read: None,
            cache_write: None,
            cache_write1h: None,
            tiers: None,
            tier_mode: None,
        }
    }
}

/// A single loaded catalog: a source name, its fetch time (`None` for
/// programmatic overrides, which have no fetch time), and the price map.
/// Mirrors `PricingCatalog`. The price map preserves insertion order so
/// same-catalog lookup priority (provider-qualified key before the bare id in
/// models.dev) matches the TS `Map` iteration order.
#[derive(Debug, Clone)]
pub struct PricingCatalog {
    pub source: String,
    pub fetched_at: Option<String>,
    pub prices: PriceMap,
}

/// An insertion-ordered `String -> ModelPrice` map, mirroring the JS `Map`
/// whose iteration order the matcher and same-catalog priority rely on.
#[derive(Debug, Clone, Default)]
pub struct PriceMap {
    order: Vec<String>,
    map: std::collections::HashMap<String, ModelPrice>,
}

impl PriceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `key -> price`, appending to the iteration order only when the
    /// key is new (a JS `Map.set` on an existing key keeps its position).
    pub fn insert(&mut self, key: String, price: ModelPrice) {
        if !self.map.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.map.insert(key, price);
    }

    pub fn get(&self, key: &str) -> Option<&ModelPrice> {
        self.map.get(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// The outcome of a successful catalog lookup. Mirrors `PriceHit`.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceHit {
    pub model: String,
    /// The catalog key that matched, which may differ from the requested name.
    pub key: String,
    pub price: ModelPrice,
    pub source: String,
    pub fetched_at: Option<String>,
}

/// The outcome of a failed lookup. Mirrors `PriceMiss`. `reason`/`key` are set
/// together only for the `zeroCost` case (a placeholder all-zero catalog entry).
#[derive(Debug, Clone, PartialEq)]
pub struct PriceMiss {
    pub model: String,
    pub attempted: Vec<String>,
    /// `Some("zeroCost")` with `key` set when the best match was a placeholder.
    pub reason: Option<String>,
    pub key: Option<String>,
}

/// A lookup result: hit or miss. Mirrors `PriceLookup`.
#[derive(Debug, Clone, PartialEq)]
pub enum PriceLookup {
    Hit(PriceHit),
    Miss(PriceMiss),
}

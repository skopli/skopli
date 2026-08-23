//! Pricing layer. Faithful port of `src/pricing/*` into the core, restricted to
//! the DATA-LEVEL seam: the core accepts pre-fetched catalogs (source +
//! fetchedAt + prices) and does matching + cost math only. Network fetching and
//! the on-disk TTL cache live in the facades (Node keeps `sources.ts` in TS);
//! the source-payload parsers in
//! [`parse`] are what a facade calls after it fetches, and the conformance
//! harness feeds committed snapshots through them with ZERO network.
//!
//! The decimal-pricing upgrade is a post-parity follow-up (schema_version 2);
//! this port keeps `f64` semantics identical to the TS source, including the
//! `costUsd` summation order that the pricing gold files pin bit-exactly.

pub mod cost;
mod match_model;
pub mod parse;
pub mod sources;
pub mod types;

pub use cost::cost_usd;
pub use match_model::{candidate_keys, is_zero_cost, match_model};
pub use types::{
    ModelPrice, PriceHit, PriceLookup, PriceMap, PriceMiss, PriceTier, PricingCatalog, TierMode,
};

use std::collections::BTreeMap;

use crate::rollup::{Rollup, RollupOptions, rollup, rollup_key};
use crate::types::UsageEvent;

/// The costing mode. Mirrors `PricingMode` in src/pricing/index.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingMode {
    Calculate,
    Display,
    Auto,
}

impl PricingMode {
    /// Parse the wire name, or `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "calculate" => Some(Self::Calculate),
            "display" => Some(Self::Display),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// The pricing decision attached to a priced rollup. Mirrors the `pricing`
/// field union of `PricedRollup` in src/pricing/index.ts.
#[derive(Debug, Clone, PartialEq)]
pub enum RollupPricing {
    /// A hit with a computed/recorded `usd` and an optional tiered-aggregate
    /// flag (set when a tiered model was aggregated across >1 event/call).
    Hit {
        hit: PriceHit,
        usd: f64,
        tiered_aggregate: bool,
    },
    /// A miss, optionally carrying a `usd` (display/auto modes surface the
    /// recorded cost even when no catalog priced the model).
    Miss { miss: PriceMiss, usd: Option<f64> },
}

/// A rollup paired with its pricing decision. Mirrors `PricedRollup`.
#[derive(Debug, Clone, PartialEq)]
pub struct PricedRollup {
    pub rollup: Rollup,
    pub pricing: RollupPricing,
}

/// The pricing decision attached to a priced EVENT group. Mirrors the `pricing`
/// field union of `PricedEventGroup` in src/pricing/index.ts (`priceEvents`).
/// Unlike [`RollupPricing`] this distinguishes a group whose events all share a
/// single model (which surfaces the full hit/miss detail) from a mixed-model
/// group (which surfaces only the aggregate `usd` + the sorted model list).
#[derive(Debug, Clone, PartialEq)]
pub enum GroupPricing {
    /// Every event priced and the group had exactly one model: the sole model's
    /// hit plus the aggregate `usd`, with `tiered_aggregate` set when a tiered
    /// model was aggregated across multiple calls.
    SingleHit {
        hit: PriceHit,
        /// The group's sole model (overrides the hit's `model`).
        model: String,
        usd: f64,
        tiered_aggregate: bool,
    },
    /// Every event priced but the group spanned multiple models: the aggregate
    /// `usd` and the sorted distinct model list (no per-model detail).
    MultiHit { usd: f64, models: Vec<String> },
    /// The group had at least one unpriced model, and exactly one model overall:
    /// the sole model's miss plus an optional aggregate `usd`.
    SingleMiss { miss: PriceMiss, usd: Option<f64> },
    /// The group had at least one unpriced model across multiple models: the
    /// group key, the union of attempted keys, and an optional aggregate `usd`.
    MultiMiss {
        key: String,
        attempted: Vec<String>,
        usd: Option<f64>,
    },
    /// Defensive fallback for a rollup bucket with no matching event group (the
    /// TS `priceEvents` treats a missing group as an all-attempted miss on the
    /// bucket key). In practice every bucket comes from an event.
    Empty { key: String },
}

/// A rollup bucket paired with its event-group pricing decision. Mirrors
/// `PricedEventGroup` in src/pricing/index.ts.
#[derive(Debug, Clone, PartialEq)]
pub struct PricedEventGroup {
    pub rollup: Rollup,
    pub pricing: GroupPricing,
}

/// A single event-group accumulator, mirroring the `Group` type in `priceEvents`
/// (pricing/index.ts).
struct EventGroup {
    usd: f64,
    all_priced: bool,
    models: Vec<String>,
    hit: Option<PriceHit>,
    miss: Option<PriceMiss>,
    attempted: Vec<String>,
    multi_call: bool,
}

/// Provenance of a loaded catalog. Mirrors `CatalogInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogInfo {
    pub source: String,
    pub fetched_at: Option<String>,
    pub models: usize,
}

/// A pricing instance over a fixed set of injected catalogs. The catalogs are
/// in priority order (highest first); an `override` catalog, when present, must
/// be first (mirroring the TS `createPricing`, which prepends the override
/// map). The catalog whose prices are the override set is identified by
/// `override_index` so zero-cost skipping is bypassed for it exactly as the TS
/// `catalog.prices !== overrides` check does.
pub struct Pricing {
    catalogs: Vec<PricingCatalog>,
    /// Index of the override catalog in `catalogs`, if any. The override
    /// catalog does NOT skip zero-cost placeholders (an explicit `$0` override
    /// is intentional).
    override_index: Option<usize>,
    mode: PricingMode,
}

impl Pricing {
    /// Build a pricing instance from injected catalogs in priority order.
    /// `override_index` marks the catalog (if any) that holds programmatic
    /// overrides; its entries bypass zero-cost skipping.
    pub fn new(
        catalogs: Vec<PricingCatalog>,
        override_index: Option<usize>,
        mode: PricingMode,
    ) -> Self {
        Self {
            catalogs,
            override_index,
            mode,
        }
    }

    /// The costing mode this instance was built with.
    pub fn mode(&self) -> PricingMode {
        self.mode
    }

    /// Provenance of the loaded catalogs. Mirrors `catalogs()`.
    pub fn catalog_infos(&self) -> Vec<CatalogInfo> {
        self.catalogs
            .iter()
            .map(|c| CatalogInfo {
                source: c.source.clone(),
                fetched_at: c.fetched_at.clone(),
                models: c.prices.len(),
            })
            .collect()
    }

    /// Look up `model` across the priority-ordered catalogs. Faithful port of
    /// `lookupModel`.
    pub fn lookup_model(&self, model: &str) -> PriceLookup {
        let mut attempted: Vec<String> = Vec::new();
        let mut zero_cost_key: Option<String> = None;
        let mut flat: Option<PriceHit> = None;

        for (idx, catalog) in self.catalogs.iter().enumerate() {
            if catalog.prices.is_empty() {
                continue;
            }
            let is_override = self.override_index == Some(idx);
            let result = match_model(model, &catalog.prices, !is_override);
            match result {
                match_model::MatchResult::Hit { key, price } => {
                    let has_tiers = price.tiers.as_ref().map(|t| t.len()).unwrap_or(0) > 0;
                    let hit = PriceHit {
                        model: model.to_owned(),
                        key,
                        price,
                        source: catalog.source.clone(),
                        fetched_at: catalog.fetched_at.clone(),
                    };
                    // An override hit, or a tiered market hit, wins outright.
                    // A flat market hit is remembered but the search continues,
                    // so a later catalog can still supply a tiered price.
                    if is_override || has_tiers {
                        return PriceLookup::Hit(hit);
                    }
                    if flat.is_none() {
                        flat = Some(hit);
                    }
                }
                match_model::MatchResult::Miss {
                    attempted: catalog_attempted,
                    zero_cost_key: zck,
                } => {
                    if zero_cost_key.is_none() {
                        zero_cost_key = zck;
                    }
                    for key in catalog_attempted {
                        if !attempted.contains(&key) {
                            attempted.push(key);
                        }
                    }
                }
            }
        }

        if let Some(hit) = flat {
            return PriceLookup::Hit(hit);
        }
        if let Some(key) = zero_cost_key {
            return PriceLookup::Miss(PriceMiss {
                model: model.to_owned(),
                attempted,
                reason: Some("zeroCost".to_owned()),
                key: Some(key),
            });
        }
        PriceLookup::Miss(PriceMiss {
            model: model.to_owned(),
            attempted,
            reason: None,
            key: None,
        })
    }

    /// Price a set of rollups. Faithful port of `priceRollups`.
    pub fn price_rollups(&self, rollups: &[Rollup]) -> Vec<PricedRollup> {
        let mut out = Vec::with_capacity(rollups.len());
        for entry in rollups {
            let lookup = self.lookup_model(&entry.key);
            let recorded = entry.cost_usd;
            match lookup {
                PriceLookup::Hit(hit) => {
                    let calculated = cost_usd(&entry.tokens, &hit.price);
                    let usd = match self.mode {
                        PricingMode::Display => recorded.unwrap_or(0.0),
                        PricingMode::Auto => recorded.unwrap_or(calculated),
                        PricingMode::Calculate => calculated,
                    };
                    let has_tiers = hit.price.tiers.as_ref().map(|t| t.len()).unwrap_or(0) > 0;
                    let tier_exposed = has_tiers
                        && (entry.events > 1 || entry.calls > 1)
                        && (self.mode == PricingMode::Calculate
                            || (self.mode == PricingMode::Auto && recorded.is_none()));
                    out.push(PricedRollup {
                        rollup: entry.clone(),
                        pricing: RollupPricing::Hit {
                            hit,
                            usd,
                            tiered_aggregate: tier_exposed,
                        },
                    });
                }
                PriceLookup::Miss(miss) => {
                    let usd = match self.mode {
                        PricingMode::Display => Some(recorded.unwrap_or(0.0)),
                        PricingMode::Auto => recorded,
                        PricingMode::Calculate => None,
                    };
                    out.push(PricedRollup {
                        rollup: entry.clone(),
                        pricing: RollupPricing::Miss { miss, usd },
                    });
                }
            }
        }
        out
    }

    /// Price a batch of events grouped by the rollup dimension. Faithful port of
    /// `priceEvents` in pricing/index.ts, producing typed [`PricedEventGroup`]s.
    /// The bit-exact `usd` summation order (per-event accumulation in the input
    /// event order) matches the pricing gold and MUST NOT change.
    pub fn price_events(
        &self,
        events: &[UsageEvent],
        options: &RollupOptions,
    ) -> Vec<PricedEventGroup> {
        let grouped = rollup(events, options);
        let mode = self.mode;

        // Cache lookups per model so a repeated model is priced once.
        let mut lookups: BTreeMap<String, PriceLookup> = BTreeMap::new();
        let mut groups: BTreeMap<String, EventGroup> = BTreeMap::new();

        for event in events {
            let key = rollup_key(event, options);
            let lookup = lookups
                .entry(event.model.clone())
                .or_insert_with(|| self.lookup_model(&event.model))
                .clone();

            let group = groups.entry(key).or_insert_with(|| EventGroup {
                usd: 0.0,
                all_priced: true,
                models: Vec::new(),
                hit: None,
                miss: None,
                attempted: Vec::new(),
                multi_call: false,
            });

            if !group.models.contains(&event.model) {
                group.models.push(event.model.clone());
            }
            if let Some(calls) = event.calls
                && calls > 1
            {
                group.multi_call = true;
            }
            let recorded = event.cost_usd.filter(|c| c.is_finite());

            match lookup {
                PriceLookup::Hit(hit) => {
                    let calculated = cost_usd(&event.tokens, &hit.price);
                    group.usd += match mode {
                        PricingMode::Display => recorded.unwrap_or(0.0),
                        PricingMode::Auto => recorded.unwrap_or(calculated),
                        PricingMode::Calculate => calculated,
                    };
                    if group.hit.is_none() {
                        group.hit = Some(hit);
                    }
                }
                PriceLookup::Miss(miss) => {
                    group.all_priced = false;
                    group.usd += match mode {
                        PricingMode::Display | PricingMode::Auto => recorded.unwrap_or(0.0),
                        PricingMode::Calculate => 0.0,
                    };
                    if group.miss.is_none() {
                        group.miss = Some(miss.clone());
                    }
                    for attempt in &miss.attempted {
                        if !group.attempted.contains(attempt) {
                            group.attempted.push(attempt.clone());
                        }
                    }
                }
            }
        }

        let display = mode == PricingMode::Display;
        let auto = mode == PricingMode::Auto;

        let mut out: Vec<PricedEventGroup> = Vec::with_capacity(grouped.len());
        for entry in grouped {
            let pricing = match groups.get(&entry.key) {
                Some(group) if group.all_priced && group.hit.is_some() => {
                    let hit = group.hit.as_ref().unwrap();
                    let single = group.models.len() == 1;
                    let tier_exposed = group.multi_call
                        && single
                        && hit.price.tiers.as_ref().map(|t| t.len()).unwrap_or(0) > 0
                        && !display;
                    if single {
                        let model = group
                            .models
                            .first()
                            .cloned()
                            .unwrap_or_else(|| hit.model.clone());
                        GroupPricing::SingleHit {
                            hit: hit.clone(),
                            model,
                            usd: group.usd,
                            tiered_aggregate: tier_exposed,
                        }
                    } else {
                        let mut models = group.models.clone();
                        models.sort();
                        GroupPricing::MultiHit {
                            usd: group.usd,
                            models,
                        }
                    }
                }
                None => GroupPricing::Empty {
                    key: entry.key.clone(),
                },
                Some(group) => {
                    let usd = if display || auto {
                        Some(group.usd)
                    } else {
                        None
                    };
                    if group.models.len() == 1 && group.miss.is_some() {
                        GroupPricing::SingleMiss {
                            miss: group.miss.as_ref().unwrap().clone(),
                            usd,
                        }
                    } else {
                        GroupPricing::MultiMiss {
                            key: entry.key.clone(),
                            attempted: group.attempted.clone(),
                            usd,
                        }
                    }
                }
            };
            out.push(PricedEventGroup {
                rollup: entry,
                pricing,
            });
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TokenCounts;

    fn tokens(input: u64, output: u64) -> TokenCounts {
        TokenCounts {
            input,
            output,
            cache_read: 0,
            cache_write: 0,
            cache_write1h: None,
            reasoning: 0,
        }
    }

    #[test]
    fn flat_cost_matches_hand_math() {
        // input 1000 @ 1.25/M, output 500 @ 10/M
        let price = ModelPrice::flat(1.25, 10.0);
        let usd = cost_usd(&tokens(1000, 500), &price);
        assert_eq!(usd, (1000.0 * 1.25 + 500.0 * 10.0) / 1_000_000.0);
    }

    #[test]
    fn candidate_alias_bedrock_claude() {
        let keys = candidate_keys("us.anthropic.claude-sonnet-4-5-20250929-v1:0");
        assert!(keys.contains(&"anthropic/claude-sonnet-4.5".to_owned()));
    }

    #[test]
    fn zero_cost_placeholder() {
        let mut p = ModelPrice::flat(0.0, 0.0);
        assert!(is_zero_cost(&p));
        p.input = 1.0;
        assert!(!is_zero_cost(&p));
    }
}

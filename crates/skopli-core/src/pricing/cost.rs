//! Cost math. Faithful port of `costUsd` / `rateSchedule` / `marginalCost` in
//! `src/pricing/index.ts`.
//!
//! Every `f64` operation is performed in the SAME ORDER as the TS source:
//! summation order matters for bit-exact float equality, which the pricing gold
//! files demand (the hermetic pricing snapshots).
//! Token counters arrive as `u64` and are cast to `f64` at the exact points the
//! TS reads them as numbers.

use crate::types::TokenCounts;

use super::types::{ModelPrice, TierMode};

/// A marginal-rate breakpoint. Mirrors the anonymous `Breakpoint` type.
struct Breakpoint {
    threshold: f64,
    rate: f64,
}

/// Faithful port of `marginalCost`. `tokens` is the whole-request context
/// footprint; `base` is the per-token rate below the first breakpoint.
fn marginal_cost(tokens: f64, base: f64, breakpoints: &[Breakpoint]) -> f64 {
    let mut usd = 0.0_f64;
    let mut prev = 0.0_f64;
    let mut rate = base;
    for point in breakpoints {
        if tokens <= point.threshold {
            break;
        }
        usd += (point.threshold - prev) * rate;
        prev = point.threshold;
        rate = point.rate;
    }
    usd + (tokens - prev) * rate
}

/// The per-level effective rates. Mirrors `EffectiveRates`.
#[derive(Clone, Copy)]
struct EffectiveRates {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    /// Rate for the 1h-TTL portion of cache writes: the catalog's above-1hr rate
    /// when present, else this level's `input * 2` (Anthropic's 1h multiplier).
    cache_write1h: f64,
}

/// A rate step keyed by its context threshold. Mirrors `RateStep`.
struct RateStep {
    threshold: f64,
    rates: EffectiveRates,
}

/// Faithful port of `rateSchedule`: the base rates plus a sorted list of
/// tier steps, each inheriting cacheRead/cacheWrite from the previous level.
fn rate_schedule(price: &ModelPrice) -> (EffectiveRates, Vec<RateStep>) {
    // `[...tiers].sort((a, b) => a.threshold - b.threshold)` - a stable numeric
    // sort of a clone.
    let mut tiers: Vec<&super::types::PriceTier> =
        price.tiers.as_deref().unwrap_or(&[]).iter().collect();
    tiers.sort_by(|a, b| {
        a.threshold
            .partial_cmp(&b.threshold)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut read = price.cache_read;
    let mut write = price.cache_write;
    let base = EffectiveRates {
        input: price.input,
        output: price.output,
        cache_read: read.unwrap_or(price.input),
        cache_write: write.unwrap_or(price.input),
        cache_write1h: price.cache_write1h.unwrap_or(price.input * 2.0),
    };
    let mut steps = Vec::with_capacity(tiers.len());
    for tier in tiers {
        read = tier.cache_read.or(read);
        write = tier.cache_write.or(write);
        steps.push(RateStep {
            threshold: tier.threshold,
            rates: EffectiveRates {
                input: tier.input,
                output: tier.output,
                cache_read: read.unwrap_or(tier.input),
                cache_write: write.unwrap_or(tier.input),
                cache_write1h: tier.cache_write1h.unwrap_or(tier.input * 2.0),
            },
        });
    }
    (base, steps)
}

/// Faithful port of `costUsd`. Returns USD for `tokens` under `price`,
/// reproducing the TS arithmetic order term-for-term.
pub fn cost_usd(tokens: &TokenCounts, price: &ModelPrice) -> f64 {
    // reasoning tokens bill as output
    let output = tokens.output as f64 + tokens.reasoning as f64;
    let (base, steps) = rate_schedule(price);
    // tier-selection context is the whole request footprint
    let context = tokens.input as f64 + tokens.cache_read as f64 + tokens.cache_write as f64;

    // The 1h write split: TS reads `tokens.cacheWrite1h` (number | undefined),
    // treats non-finite/absent as 0, then clamps into [0, cacheWrite].
    let reported1h = tokens.cache_write1h;
    let finite1h = match reported1h {
        Some(v) if (v as f64).is_finite() => v as f64,
        _ => 0.0,
    };
    let write1h = finite1h.max(0.0).min((tokens.cache_write as f64).max(0.0));
    let write5m = tokens.cache_write as f64 - write1h;

    // split-aware cache-write cost at a rate level
    let cache_write_cost =
        |r: &EffectiveRates| -> f64 { write5m * r.cache_write + write1h * r.cache_write1h };
    let bill = |r: &EffectiveRates| -> f64 {
        (tokens.input as f64 * r.input
            + output * r.output
            + tokens.cache_read as f64 * r.cache_read
            + cache_write_cost(r))
            / 1_000_000.0
    };

    if steps.is_empty() {
        return bill(&base);
    }
    if price.tier_mode == Some(TierMode::WholeRequest) {
        let mut active = base;
        for step in &steps {
            if context <= step.threshold {
                break;
            }
            active = step.rates;
        }
        return bill(&active);
    }

    // marginal mode: blend input + cacheRead + cacheWrite (split-aware) into a
    // per-context-token rate, run the marginal breakpoints, then add output.
    let blend = |r: &EffectiveRates| -> f64 {
        if context == 0.0 {
            0.0
        } else {
            (tokens.input as f64 * r.input
                + tokens.cache_read as f64 * r.cache_read
                + cache_write_cost(r))
                / context
        }
    };
    let breakpoints: Vec<Breakpoint> = steps
        .iter()
        .map(|s| Breakpoint {
            threshold: s.threshold,
            rate: blend(&s.rates),
        })
        .collect();
    (marginal_cost(context, blend(&base), &breakpoints) + output * base.output) / 1_000_000.0
}

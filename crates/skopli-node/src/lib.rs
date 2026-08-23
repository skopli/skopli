//! napi-rs v3 binding for `skopli-core`. This is the native engine behind
//! the root `skopli` npm package: the TS facade (src/*.ts) keeps the public
//! API, the platform-specific parts (date-bounds tz math, harness detection,
//! pricing source loading / fetch / cache), and delegates the pure data + math
//! work to the functions here.
//!
//! Boundary shape: every call takes/returns JSON strings (`serde_json`), so the
//! TS side owns (de)serialization to its own types and the ABI stays a single
//! stable string-in/string-out contract. Errors are surfaced as JS `Error`s
//! (napi `Error`), which the facade rethrows unchanged.
//!
//! Async: `readHarness` and the `Pricing` lookups run on the libuv threadpool
//! via napi `AsyncTask`, matching the `Promise`-returning TS surface. `detect`
//! is not here (it stays a synchronous dir-probe in TS); `rollup` / `costUsd` /
//! `defaultCacheDir` / `supportedHarnesses` are synchronous.

mod parse;
mod pipeline;
mod pricing_opts;
mod wire;

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::{Env, Task};
use napi_derive::napi;
use serde_json::Value;

/// Parse a JSON string argument into a `serde_json::Value`, mapping a parse
/// failure to a JS `Error`.
fn parse_json(arg: &str) -> Result<Value> {
    serde_json::from_str(arg).map_err(|e| Error::from_reason(format!("invalid JSON argument: {e}")))
}

/// Serialize a `serde_json::Value` result to a JSON string.
fn to_json(value: &Value) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|e| Error::from_reason(format!("failed to serialize result: {e}")))
}

/// Map an internal `Result<_, String>` domain error into a JS `Error`.
fn domain<T>(r: std::result::Result<T, String>) -> Result<T> {
    r.map_err(Error::from_reason)
}

// ---------------------------------------------------------------------------
// Synchronous surface
// ---------------------------------------------------------------------------

/// The harness ids the native core can read. The facade routes any other
/// (still-TS) harness to the TS reader layer.
#[napi(js_name = "supportedHarnesses")]
pub fn supported_harnesses() -> Vec<String> {
    pipeline::supported_harnesses()
}

/// The default on-disk pricing cache directory (platform-specific layout).
#[napi(js_name = "defaultCacheDir")]
pub fn default_cache_dir() -> String {
    parse::default_cache_dir()
}

/// Roll up a batch of events. `events_json` is a `UsageEvent[]`; `options_json`
/// is `{ by, tz? }`. Returns a `Rollup[]` JSON string.
#[napi(js_name = "rollup")]
pub fn rollup(events_json: String, options_json: String) -> Result<String> {
    let events = domain(parse::parse_events(&parse_json(&events_json)?))?;
    let options = parse_json(&options_json)?;
    let out = domain(pipeline::rollup_events(&events, &options))?;
    to_json(&out)
}

/// Compute the USD cost of a token bundle at a price. `tokens_json` is a
/// `TokenCounts`; `price_json` is a `ModelPrice`. Returns the number.
#[napi(js_name = "costUsd")]
pub fn cost_usd(tokens_json: String, price_json: String) -> Result<f64> {
    let tokens = domain(parse::parse_tokens(&parse_json(&tokens_json)?))?;
    let price = domain(parse::parse_price(&parse_json(&price_json)?))?;
    Ok(skopli_core::pricing::cost_usd(&tokens, &price))
}

// ---------------------------------------------------------------------------
// readHarness: async per-harness gather
// ---------------------------------------------------------------------------

/// AsyncTask reading one harness off the libuv threadpool.
pub struct ReadHarnessTask {
    harness: String,
    options: Value,
}

#[napi]
impl Task for ReadHarnessTask {
    type Output = Value;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        Ok(pipeline::read_harness(&self.harness, &self.options))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        to_json(&output)
    }
}

/// Read a single harness's raw usage under `{home, env}` (from `options_json`).
/// Returns a Promise of a `{events, diagnostics, skipped}` JSON string. The TS
/// facade calls this once per addon-supported harness, then applies its shared
/// timestamp / since / until / subagent filter loop over the union.
#[napi(js_name = "readHarness", ts_return_type = "Promise<string>")]
pub fn read_harness(harness: String, options_json: String) -> Result<AsyncTask<ReadHarnessTask>> {
    let options = parse_json(&options_json)?;
    Ok(AsyncTask::new(ReadHarnessTask { harness, options }))
}

// ---------------------------------------------------------------------------
// Pricing: a handle wrapping a core Pricing built from injected catalogs
// ---------------------------------------------------------------------------

/// A pricing engine over a fixed set of TS-loaded catalogs. Constructed from the
/// facade's `pricingNew` options (mode + overrides + pre-fetched catalogs); the
/// facade owns fetching / caching / refresh and rebuilds a new handle whenever
/// its catalogs change (e.g. a TTL reload), so this holds an immutable snapshot.
#[napi(js_name = "Pricing")]
pub struct JsPricing {
    inner: Arc<skopli_core::pricing::Pricing>,
}

/// A boxed pricing operation: takes the core `Pricing` and produces a JSON
/// value or a JS error. Boxed so one task type serves every `Pricing` method.
type PricingOp = Box<dyn FnOnce(&skopli_core::pricing::Pricing) -> Result<Value> + Send>;

/// AsyncTask running a pricing operation off the threadpool.
pub struct PricingTask {
    inner: Arc<skopli_core::pricing::Pricing>,
    op: Option<PricingOp>,
}

#[napi]
impl Task for PricingTask {
    type Output = Value;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        let op = self
            .op
            .take()
            .ok_or_else(|| Error::from_reason("pricing task already run"))?;
        op(&self.inner)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        to_json(&output)
    }
}

#[napi]
impl JsPricing {
    /// Build a pricing engine. `options_json` is
    /// `{ mode?, overrides?, catalogs? }` (see `pricing_opts`).
    #[napi(constructor)]
    pub fn new(options_json: String) -> Result<Self> {
        let options = parse_json(&options_json)?;
        let pricing = domain(pricing_opts::pricing_from_options(&options))?;
        Ok(Self {
            inner: Arc::new(pricing),
        })
    }

    fn task(
        &self,
        op: impl FnOnce(&skopli_core::pricing::Pricing) -> Result<Value> + Send + 'static,
    ) -> AsyncTask<PricingTask> {
        AsyncTask::new(PricingTask {
            inner: Arc::clone(&self.inner),
            op: Some(Box::new(op)),
        })
    }

    /// Provenance of the loaded catalogs: `CatalogInfo[]` JSON.
    #[napi(js_name = "catalogs", ts_return_type = "Promise<string>")]
    pub fn catalogs(&self) -> AsyncTask<PricingTask> {
        self.task(|p| {
            let infos: Vec<Value> = p
                .catalog_infos()
                .iter()
                .map(wire::catalog_info_json)
                .collect();
            Ok(Value::Array(infos))
        })
    }

    /// Look up a model: a `PriceLookup` (`PriceHit | PriceMiss`) JSON.
    #[napi(js_name = "lookupModel", ts_return_type = "Promise<string>")]
    pub fn lookup_model(&self, model: String) -> AsyncTask<PricingTask> {
        self.task(move |p| Ok(wire::price_lookup_json(&p.lookup_model(&model))))
    }

    /// Price a set of rollups: `PricedRollup[]` JSON. `rollups_json` is a
    /// `Rollup[]`.
    #[napi(js_name = "priceRollups", ts_return_type = "Promise<string>")]
    pub fn price_rollups(&self, rollups_json: String) -> Result<AsyncTask<PricingTask>> {
        let rollups = parse_json(&rollups_json)?;
        Ok(self.task(move |p| domain(pipeline::price_rollups(p, &rollups))))
    }

    /// Price a batch of events grouped by a rollup dimension:
    /// `PricedEventGroup[]` JSON. `events_json` is a `UsageEvent[]`;
    /// `options_json` is `{ by, tz? }`.
    #[napi(js_name = "priceEvents", ts_return_type = "Promise<string>")]
    pub fn price_events(
        &self,
        events_json: String,
        options_json: String,
    ) -> Result<AsyncTask<PricingTask>> {
        let events = domain(parse::parse_events(&parse_json(&events_json)?))?;
        let options = parse_json(&options_json)?;
        Ok(self.task(move |p| domain(pipeline::price_events(p, &events, &options))))
    }
}

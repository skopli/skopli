//! magnus binding for `skopli-core`. This is the native engine behind the
//! typed `skopli` Ruby gem: the pure-Ruby facade (`lib/skopli`) keeps
//! the idiomatic surface (snake_case module functions, `Data.define` value
//! classes, symbol dimensions like `:claude` / `:day`, keyword-args, the
//! `Skopli::Error < StandardError` hierarchy), and delegates the pure data +
//! math work to the functions here.
//!
//! Boundary shape: every call takes/returns JSON strings (`serde_json`), so the
//! Ruby side owns (de)serialization to its own types and the ABI stays a single
//! stable string-in/string-out contract - the same JSON-boundary design the C
//! ABI, node, and PyO3 bindings use (this crate binds core DIRECTLY, not the C
//! ABI).
//!
//! GVL: sync-only v1. Every entry point releases the GVL via
//! [`gvl::without_gvl`] around the JSON parse + core work, so callers can
//! parallelise with Ruby threads. Only argument/result marshalling touches the
//! VM.
//!
//! Errors: parse/domain failures raise a single native `Skopli::NativeError`
//! carrying a `kind:` tag on its message (`invalid_argument` | `catalog` |
//! `internal`); the Ruby facade re-raises it into the typed
//! `InvalidArgumentError` / `CatalogError` hierarchy. Keeping one native
//! exception leaves the Rust side idiom-free.

mod gvl;
mod modules;
mod parse;
mod pipeline;
mod pricing_opts;
mod wire;

use magnus::value::Lazy;
use magnus::{Error, ExceptionClass, RClass, RModule, Ruby, function, method, prelude::*};
use serde_json::Value;

use modules::{ErrKind, RubyResultExt, flatten_fallible, flatten_infallible};

/// The `Skopli` module, memoised for the lifetime of the VM.
static SKOPLI_MODULE: Lazy<RModule> =
    Lazy::new(|ruby| ruby.define_module("Skopli").expect("define Skopli module"));

/// The single native exception class `Skopli::NativeError < StandardError`.
/// The Ruby facade catches this and re-raises the typed subclass by inspecting
/// the `kind:` tag on the message.
static NATIVE_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let module = ruby.get_inner(&SKOPLI_MODULE);
    module
        .define_error("NativeError", ruby.exception_standard_error())
        .expect("define Skopli::NativeError")
});

/// Accessor used by the error glue to build a raised exception.
pub(crate) fn native_error_class(ruby: &Ruby) -> ExceptionClass {
    ruby.get_inner(&NATIVE_ERROR)
}

/// Parse a JSON string argument into a `serde_json::Value`, mapping a parse
/// failure to an invalid-argument domain error.
fn parse_json(arg: &str) -> Result<Value, DomainError> {
    serde_json::from_str(arg).map_err(|e| DomainError {
        kind: ErrKind::InvalidArgument,
        message: format!("invalid JSON argument: {e}"),
    })
}

/// Serialize a `serde_json::Value` result to a JSON string.
fn to_json(value: &Value) -> Result<String, DomainError> {
    serde_json::to_string(value).map_err(|e| DomainError {
        kind: ErrKind::Internal,
        message: format!("failed to serialize result: {e}"),
    })
}

/// An internal domain error with a facade-facing `kind`.
pub(crate) struct DomainError {
    pub(crate) kind: ErrKind,
    pub(crate) message: String,
}

impl DomainError {
    fn invalid(message: String) -> Self {
        Self {
            kind: ErrKind::InvalidArgument,
            message,
        }
    }
    fn catalog(message: String) -> Self {
        Self {
            kind: ErrKind::Catalog,
            message,
        }
    }
}

// ---------------------------------------------------------------------------
// Module functions (all sync, all release the GVL)
// ---------------------------------------------------------------------------

/// The default on-disk pricing cache directory (platform-specific layout).
fn default_cache_dir(ruby: &Ruby) -> Result<String, Error> {
    flatten_infallible(gvl::without_gvl(parse::default_cache_dir)).into_ruby_err(ruby)
}

/// Detect which harnesses have readable data under `{home?, env?}` (from
/// `options_json`). Returns a `{supported, unsupported}` JSON string.
fn detect_harnesses(ruby: &Ruby, options_json: String) -> Result<String, Error> {
    flatten_fallible(gvl::without_gvl(move || {
        let value = parse_json(&options_json)?;
        to_json(&pipeline::detect(&value))
    }))
    .into_ruby_err(ruby)
}

/// Read usage across the selected harnesses under `{home?, env?, harnesses?,
/// since?, until?, tz?, subagents?}` (from `options_json`). Returns an
/// `{events, diagnostics, skipped}` JSON string.
fn read_usage(ruby: &Ruby, options_json: String) -> Result<String, Error> {
    flatten_fallible(gvl::without_gvl(move || {
        let value = parse_json(&options_json)?;
        let options = pipeline::parse_read_options(&value).map_err(DomainError::invalid)?;
        to_json(&pipeline::read_usage(&options))
    }))
    .into_ruby_err(ruby)
}

/// Roll up an event array by a dimension. `events_json` is a `UsageEvent[]`;
/// `options_json` is `{by, tz?}`. Returns a `Rollup[]` JSON string.
fn rollup(ruby: &Ruby, events_json: String, options_json: String) -> Result<String, Error> {
    flatten_fallible(gvl::without_gvl(move || {
        let events =
            parse::parse_events(&parse_json(&events_json)?).map_err(DomainError::invalid)?;
        let options = parse_json(&options_json)?;
        let out = pipeline::rollup_events(&events, &options).map_err(DomainError::invalid)?;
        to_json(&out)
    }))
    .into_ruby_err(ruby)
}

/// Compute the USD cost of a token bundle under a flat/tiered price.
/// `tokens_json` is a `TokenCounts`; `price_json` is a `ModelPrice`. Returns the
/// number.
fn cost_usd(ruby: &Ruby, tokens_json: String, price_json: String) -> Result<f64, Error> {
    flatten_fallible(gvl::without_gvl(move || {
        let tokens =
            parse::parse_tokens(&parse_json(&tokens_json)?).map_err(DomainError::invalid)?;
        let price = parse::parse_price(&parse_json(&price_json)?).map_err(DomainError::invalid)?;
        Ok(skopli_core::pricing::cost_usd(&tokens, &price))
    }))
    .into_ruby_err(ruby)
}

// ---------------------------------------------------------------------------
// Pricing handle
// ---------------------------------------------------------------------------

/// A pricing engine over a fixed set of facade-loaded catalogs. Constructed from
/// `create_pricing` options (mode + overrides + pre-fetched catalogs); the facade
/// owns fetching / caching / refresh and rebuilds a new handle whenever its
/// catalogs change, so this holds an immutable snapshot. magnus GC-manages the
/// wrapper and frees it in its mark/free hooks - no explicit close needed.
#[magnus::wrap(class = "Skopli::NativePricing", free_immediately, size)]
struct NativePricing {
    inner: skopli_core::pricing::Pricing,
}

impl NativePricing {
    /// Build a pricing engine. `options_json` is `{mode?, overrides?, catalogs?}`.
    fn new(ruby: &Ruby, options_json: String) -> Result<Self, Error> {
        let pricing = flatten_fallible(gvl::without_gvl(move || {
            let value = parse_json(&options_json)?;
            pricing_opts::pricing_from_options(&value).map_err(DomainError::catalog)
        }))
        .into_ruby_err(ruby)?;
        Ok(Self { inner: pricing })
    }

    /// Provenance of the loaded catalogs: `CatalogInfo[]` JSON.
    fn catalogs(ruby: &Ruby, rb_self: &Self) -> Result<String, Error> {
        flatten_fallible(gvl::without_gvl(|| {
            let infos: Vec<Value> = rb_self
                .inner
                .catalog_infos()
                .iter()
                .map(wire::catalog_info_json)
                .collect();
            to_json(&Value::Array(infos))
        }))
        .into_ruby_err(ruby)
    }

    /// Look up a model: a `PriceHit | PriceMiss` JSON.
    fn lookup_model(ruby: &Ruby, rb_self: &Self, model: String) -> Result<String, Error> {
        flatten_fallible(gvl::without_gvl(|| {
            to_json(&wire::price_lookup_json(
                &rb_self.inner.lookup_model(&model),
            ))
        }))
        .into_ruby_err(ruby)
    }

    /// Price a set of rollups: `PricedRollup[]` JSON. `rollups_json` is a
    /// `Rollup[]`.
    fn price_rollups(ruby: &Ruby, rb_self: &Self, rollups_json: String) -> Result<String, Error> {
        flatten_fallible(gvl::without_gvl(|| {
            let rollups = parse_json(&rollups_json)?;
            let out =
                pipeline::price_rollups(&rb_self.inner, &rollups).map_err(DomainError::catalog)?;
            to_json(&out)
        }))
        .into_ruby_err(ruby)
    }

    /// Price a batch of events grouped by a rollup dimension:
    /// `PricedEventGroup[]` JSON. `events_json` is a `UsageEvent[]`;
    /// `options_json` is `{by, tz?}`.
    fn price_events(
        ruby: &Ruby,
        rb_self: &Self,
        events_json: String,
        options_json: String,
    ) -> Result<String, Error> {
        flatten_fallible(gvl::without_gvl(|| {
            let events =
                parse::parse_events(&parse_json(&events_json)?).map_err(DomainError::invalid)?;
            let options = parse_json(&options_json)?;
            let out = pipeline::price_events(&rb_self.inner, &events, &options)
                .map_err(DomainError::catalog)?;
            to_json(&out)
        }))
        .into_ruby_err(ruby)
    }
}

/// Entry point invoked by Ruby when the native extension is `require`d
/// (`Init_skopli` via the `#[magnus::init]` shim). Defines the `Skopli`
/// module, the native module-functions, the native exception class, and the
/// `NativePricing` class - all under the `Skopli::Native*` namespace the
/// pure-Ruby facade wraps.
#[magnus::init(name = "skopli")]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.get_inner(&SKOPLI_MODULE);
    // Force the exception class to be defined at load time.
    let _ = ruby.get_inner(&NATIVE_ERROR);

    module.define_module_function("native_default_cache_dir", function!(default_cache_dir, 0))?;
    module.define_module_function("native_detect_harnesses", function!(detect_harnesses, 1))?;
    module.define_module_function("native_read_usage", function!(read_usage, 1))?;
    module.define_module_function("native_rollup", function!(rollup, 2))?;
    module.define_module_function("native_cost_usd", function!(cost_usd, 2))?;
    module.const_set("NATIVE_VERSION", env!("CARGO_PKG_VERSION"))?;

    let pricing_class: RClass = module.define_class("NativePricing", ruby.class_object())?;
    pricing_class.define_singleton_method("new", function!(NativePricing::new, 1))?;
    pricing_class.define_method("catalogs", method!(NativePricing::catalogs, 0))?;
    pricing_class.define_method("lookup_model", method!(NativePricing::lookup_model, 1))?;
    pricing_class.define_method("price_rollups", method!(NativePricing::price_rollups, 1))?;
    pricing_class.define_method("price_events", method!(NativePricing::price_events, 2))?;

    Ok(())
}

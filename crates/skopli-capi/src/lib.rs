//! `skopli-capi` - the normative C ABI over `skopli-core`.
//!
//! Exactly 17 `extern "C"` functions prefixed `ag_`:
//! 6 lifecycle/meta + 11 pipeline. Bulk payloads AND options both cross as
//! nullable UTF-8 JSON buffers (`null`/empty = all defaults); library-owned
//! outputs are `AgBuf`s freed by [`ag_buf_free`] or C strings freed by
//! [`ag_string_free`]. Every fallible function returns [`AgStatus`] and records
//! detail in the thread-local [`ag_last_error_message`]. No callbacks ever cross
//! the ABI; `AgPricing` is the only opaque handle.
//!
//! ## Panic safety
//! Every entry point runs its body inside a `catch_unwind` guard
//! ([`abi::guard`]/[`abi::guard_value`]); a panic is converted to
//! [`AgStatus::Internal`] plus an error message and NEVER unwinds across the FFI
//! boundary.
//!
//! ## Thread safety
//! The library holds no process-global mutable state beyond the per-thread error
//! message, so all functions are safe to call concurrently. An `AgPricing`
//! handle holds only an immutable catalog set and is `Send + Sync`; a caller
//! must still not free a handle while another thread is using it.

mod abi;
mod json;
mod pricing_opts;

use std::ffi::{CString, c_char};

use serde_json::Value;
use skopli_core::pricing::Pricing;
use skopli_core::types::UsageEvent;

pub use abi::{AgBuf, AgStatus, ag_buf_free, ag_last_error_message, ag_string_free};

/// Net-gated, `doc(hidden)` re-exports so the crate's integration smoke test can
/// prove the built-in fetch path against a local server. These are NOT part of
/// the C ABI, the `ag_pricing_new` options grammar, or `skopli.h`; they exist
/// only to inject test source URLs into the shared options parser.
#[cfg(feature = "net")]
#[doc(hidden)]
pub use pricing_opts::{SourceUrls, pricing_from_options_with_urls};

use abi::{AgError, entry, guard, guard_value};
use skopli_wire::{parse, pipeline, read, wire};

/// The ABI version. Bump on any breaking change to the exported symbols or their
/// contracts. Additive JSON keys do NOT bump this.
const ABI_VERSION: u32 = 1;

/// The conformance schema_version this build produces (gold envelopes are v1).
const SCHEMA_VERSION: u32 = 1;

/// The library semver string, from the crate version.
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

// ---------------------------------------------------------------------------
// Lifecycle / meta (6)
// ---------------------------------------------------------------------------

/// The ABI version integer. Bumped on a breaking ABI change.
#[unsafe(no_mangle)]
pub extern "C" fn ag_abi_version() -> u32 {
    guard_value(0, || ABI_VERSION)
}

/// The conformance `schema_version` this build emits.
#[unsafe(no_mangle)]
pub extern "C" fn ag_schema_version() -> u32 {
    guard_value(0, || SCHEMA_VERSION)
}

/// A pointer to a static NUL-terminated library semver string. NEVER freed.
#[unsafe(no_mangle)]
pub extern "C" fn ag_version() -> *const c_char {
    guard_value(std::ptr::null(), || VERSION.as_ptr() as *const c_char)
}

// `ag_buf_free`, `ag_string_free`, `ag_last_error_message` are defined in `abi`
// and re-exported above (fns 4-6).

// ---------------------------------------------------------------------------
// Pipeline (11)
// ---------------------------------------------------------------------------

/// Detect which registered harnesses have readable data reachable from the
/// context in `opts_json` (`{home?, env?}`; null = process defaults). Writes a
/// `{supported: string[], unsupported: string[]}` JSON buffer to `*out`.
///
/// # Safety
/// `opts_json`/`opts_len` describe a readable buffer or (null, 0); `out` is a
/// valid writable `*mut AgBuf`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_detect_harnesses(
    opts_json: *const c_char,
    opts_len: usize,
    out: *mut AgBuf,
) -> AgStatus {
    entry(|| {
        let opts = unsafe { json::read_json_opt(opts_json, opts_len) }?;
        write_json(out, read::detect(&opts))
    })
}

/// Read usage across the selected harnesses. `opts_json`
/// (`{home?, env?, harnesses?, since?, until?, tz?, subagents?}`; null =
/// defaults) drives detection/filtering. Writes an
/// `{events, diagnostics, skipped}` JSON buffer to `*out`.
///
/// # Safety
/// See [`ag_detect_harnesses`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_read_usage(
    opts_json: *const c_char,
    opts_len: usize,
    out: *mut AgBuf,
) -> AgStatus {
    entry(|| {
        let opts = unsafe { json::read_json_opt(opts_json, opts_len) }?;
        let options = read::parse_read_options(&opts).map_err(AgError::invalid)?;
        write_json(out, read::read_usage(&options))
    })
}

/// Roll up an event array by a dimension. `events_json` is a `UsageEvent[]`;
/// `opts_json` is `{by, tz?, blockMs?}`. Writes a `Rollup[]` JSON buffer to
/// `*out`.
///
/// # Safety
/// Both JSON args describe readable buffers or (null, 0); `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_rollup(
    events_json: *const c_char,
    events_len: usize,
    opts_json: *const c_char,
    opts_len: usize,
    out: *mut AgBuf,
) -> AgStatus {
    entry(|| {
        let events = unsafe { read_events(events_json, events_len) }?;
        let opts = unsafe { json::read_json_opt(opts_json, opts_len) }?;
        let value = pipeline::rollup_events(&events, &opts).map_err(AgError::invalid)?;
        write_json(out, value)
    })
}

/// Compute the USD cost of a token bundle under a flat/tiered price.
/// `tokens_json` is a `TokenCounts`; `price_json` is a `ModelPrice`. Writes the
/// result to `*out` (an `f64`).
///
/// # Safety
/// Both JSON args are non-null readable buffers; `out` is a writable `*mut f64`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_cost_usd(
    tokens_json: *const c_char,
    tokens_len: usize,
    price_json: *const c_char,
    price_len: usize,
    out: *mut f64,
) -> AgStatus {
    entry(|| {
        let out = non_null(out)?;
        let tokens_v = unsafe { json::read_json_opt(tokens_json, tokens_len) }?;
        let price_v = unsafe { json::read_json_opt(price_json, price_len) }?;
        let tokens = parse::parse_tokens(&tokens_v).map_err(AgError::invalid)?;
        let price = parse::parse_price(&price_v).map_err(AgError::invalid)?;
        unsafe { *out = skopli_core::pricing::cost_usd(&tokens, &price) };
        Ok(())
    })
}

/// Write the default on-disk cache directory (platform-specific) as a
/// library-owned C string to `*out`; free it with [`ag_string_free`].
///
/// # Safety
/// `out` is a valid writable `*mut *mut c_char`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_default_cache_dir(out: *mut *mut c_char) -> AgStatus {
    entry(|| {
        let out = non_null(out)?;
        let dir = CString::new(parse::default_cache_dir())
            .map_err(|e| AgError::internal(format!("cache dir contains interior NUL: {e}")))?;
        unsafe { *out = dir.into_raw() };
        Ok(())
    })
}

/// Construct an `AgPricing` handle from injected catalogs/overrides in
/// `opts_json` (`{mode?, overrides?, catalogs?, ...}`; null = empty). Writes the
/// handle to `*out`; free it with [`ag_pricing_free`].
///
/// The default `libskopli` performs no network I/O: it opens no socket and
/// fetches no catalog. Callers pass pre-fetched `catalogs` (a source name plus
/// either a prices map or a raw `format`+`payload`) and price against those.
/// Built-in source fetching requires the optional `net` cargo feature (the
/// separate `libskopli-full` build). When `opts_json` asks for built-in sources
/// (`builtinSources:true`) and the library was compiled without `net`, this
/// returns [`AgStatus::Catalog`] with the `NoBuiltinSources` error message
/// recorded via [`ag_last_error_message`].
///
/// # Safety
/// `opts_json`/`opts_len` describe a readable buffer or (null, 0); `out` is a
/// valid writable `*mut *mut AgPricing`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_pricing_new(
    opts_json: *const c_char,
    opts_len: usize,
    out: *mut *mut AgPricing,
) -> AgStatus {
    entry(|| {
        let out = non_null(out)?;
        let opts = unsafe { json::read_json_opt(opts_json, opts_len) }?;
        let pricing = pricing_opts::pricing_from_options(&opts).map_err(AgError::catalog)?;
        unsafe { *out = Box::into_raw(Box::new(AgPricing { inner: pricing })) };
        Ok(())
    })
}

/// Free an `AgPricing` handle. Passing null is a safe no-op.
///
/// # Safety
/// `pricing` must be a handle from [`ag_pricing_new`] not yet freed, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_pricing_free(pricing: *mut AgPricing) {
    let _ = guard(|| {
        if !pricing.is_null() {
            unsafe { drop(Box::from_raw(pricing)) };
        }
        AgStatus::Ok
    });
}

/// Price an event batch grouped by the option dimension. `events_json` is a
/// `UsageEvent[]`; `opts_json` is `{by, tz?, blockMs?}`. Writes a
/// `PricedEventGroup[]` JSON buffer to `*out`.
///
/// # Safety
/// `pricing` is a live handle; the JSON args describe readable buffers or
/// (null, 0); `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_pricing_price_events(
    pricing: *const AgPricing,
    events_json: *const c_char,
    events_len: usize,
    opts_json: *const c_char,
    opts_len: usize,
    out: *mut AgBuf,
) -> AgStatus {
    entry(|| {
        let handle = unsafe { pricing_handle(pricing) }?;
        let events = unsafe { read_events(events_json, events_len) }?;
        let opts = unsafe { json::read_json_opt(opts_json, opts_len) }?;
        let value = pipeline::price_events(handle, &events, &opts).map_err(AgError::catalog)?;
        write_json(out, value)
    })
}

/// Look up a single model's price. `model`/`model_len` describe a bare UTF-8
/// model-name buffer (NOT JSON); a null or empty buffer is an invalid argument.
/// Writes a single `PriceLookup` JSON object (a `{priced:true, ...}` hit or a
/// `{priced:false, ...}` miss) to `*out`.
///
/// # Safety
/// `pricing` is a live handle; `model`/`model_len` describe a readable buffer;
/// `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_pricing_lookup_model(
    pricing: *const AgPricing,
    model: *const c_char,
    model_len: usize,
    out: *mut AgBuf,
) -> AgStatus {
    entry(|| {
        let handle = unsafe { pricing_handle(pricing) }?;
        if model.is_null() || model_len == 0 {
            return Err(AgError::invalid("model name buffer is null or empty"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(model as *const u8, model_len) };
        let name = std::str::from_utf8(bytes)
            .map_err(|e| AgError::invalid(format!("model name is not valid UTF-8: {e}")))?;
        write_json(out, wire::price_lookup_json(&handle.lookup_model(name)))
    })
}

/// Price a set of rollups. `rollups_json`/`rollups_len` describe a `Rollup[]`
/// JSON body, the same shape [`ag_rollup`] emits. Writes a `PricedRollup[]` JSON
/// buffer to `*out`: each element is the caller's original rollup object with a
/// `pricing` field attached (a hit with an optional `tieredAggregate`, or a
/// miss). Parse/catalog failures map to [`AgStatus::Catalog`].
///
/// # Safety
/// `pricing` is a live handle; `rollups_json`/`rollups_len` describe a readable
/// buffer or (null, 0); `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_pricing_price_rollups(
    pricing: *const AgPricing,
    rollups_json: *const c_char,
    rollups_len: usize,
    out: *mut AgBuf,
) -> AgStatus {
    entry(|| {
        let handle = unsafe { pricing_handle(pricing) }?;
        let rollups = unsafe { json::read_json_opt(rollups_json, rollups_len) }?;
        let value = pipeline::price_rollups(handle, &rollups).map_err(AgError::catalog)?;
        write_json(out, value)
    })
}

/// Write the provenance of the handle's catalogs as a `CatalogInfo[]` JSON
/// buffer to `*out`.
///
/// # Safety
/// `pricing` is a live handle; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_pricing_catalog_info(
    pricing: *const AgPricing,
    out: *mut AgBuf,
) -> AgStatus {
    entry(|| {
        let handle = unsafe { pricing_handle(pricing) }?;
        let infos = handle.catalog_infos();
        write_json(
            out,
            Value::Array(infos.iter().map(wire::catalog_info_json).collect()),
        )
    })
}

// ---------------------------------------------------------------------------
// Opaque handle
// ---------------------------------------------------------------------------

/// The opaque pricing handle. Holds an immutable core [`Pricing`] instance
/// (injected catalogs + mode); constructed by [`ag_pricing_new`], freed by
/// [`ag_pricing_free`].
pub struct AgPricing {
    inner: Pricing,
}

// ---------------------------------------------------------------------------
// Internal helpers (not part of the ABI)
// ---------------------------------------------------------------------------

/// Write `value` as a JSON buffer to `*out` (the shared success path of every
/// `AgBuf`-returning function).
fn write_json(out: *mut AgBuf, value: Value) -> Result<(), AgError> {
    let out = non_null(out)?;
    let text = serde_json::to_string(&value)
        .map_err(|e| AgError::internal(format!("failed to serialize result: {e}")))?;
    unsafe { *out = AgBuf::from_string(text) };
    Ok(())
}

fn non_null<T>(out: *mut T) -> Result<*mut T, AgError> {
    if out.is_null() {
        return Err(AgError::invalid("out pointer is null"));
    }
    Ok(out)
}

/// Borrow the core engine behind an `AgPricing` handle.
///
/// # Safety
/// `pricing` is null or a handle from `ag_pricing_new` not yet freed.
unsafe fn pricing_handle<'a>(pricing: *const AgPricing) -> Result<&'a Pricing, AgError> {
    match unsafe { pricing.as_ref() } {
        Some(handle) => Ok(&handle.inner),
        None => Err(AgError::invalid("pricing handle is null")),
    }
}

/// Read a nullable JSON buffer holding a `UsageEvent[]`.
///
/// # Safety
/// See [`json::read_json_opt`].
unsafe fn read_events(ptr: *const c_char, len: usize) -> Result<Vec<UsageEvent>, AgError> {
    let value = unsafe { json::read_json_opt(ptr, len) }?;
    parse::parse_events(&value).map_err(AgError::invalid)
}

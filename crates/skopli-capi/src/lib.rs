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
mod pipeline;
mod pricing_opts;
mod wire;

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

use abi::{clear_last_error, guard, guard_value, set_last_error};

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
// Pipeline (9)
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
    with_out(out, opts_json, opts_len, |opts| {
        let value = pipeline::detect(&opts);
        Ok(value)
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
    guard(|| {
        clear_last_error();
        let opts = match unsafe { json::read_json_opt(opts_json, opts_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        let options = match pipeline::parse_read_options(&opts) {
            Ok(o) => o,
            Err(msg) => {
                set_last_error(msg);
                return AgStatus::InvalidArgument;
            }
        };
        let value = pipeline::read_usage(&options);
        write_json(out, value)
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
    guard(|| {
        clear_last_error();
        let events = match unsafe { read_events(events_json, events_len) } {
            Ok(e) => e,
            Err(s) => return s,
        };
        let opts = match unsafe { json::read_json_opt(opts_json, opts_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        match pipeline::rollup_events(&events, &opts) {
            Ok(value) => write_json(out, value),
            Err(msg) => {
                set_last_error(msg);
                AgStatus::InvalidArgument
            }
        }
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
    guard(|| {
        clear_last_error();
        if out.is_null() {
            set_last_error("out pointer is null");
            return AgStatus::InvalidArgument;
        }
        let tokens_v = match unsafe { json::read_json_opt(tokens_json, tokens_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        let price_v = match unsafe { json::read_json_opt(price_json, price_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        let tokens: skopli_core::types::TokenCounts =
            match skopli_wire::parse::parse_tokens(&tokens_v) {
                Ok(t) => t,
                Err(msg) => {
                    set_last_error(msg);
                    return AgStatus::InvalidArgument;
                }
            };
        let price = match skopli_wire::parse::parse_price(&price_v) {
            Ok(p) => p,
            Err(msg) => {
                set_last_error(msg);
                return AgStatus::InvalidArgument;
            }
        };
        let usd = skopli_core::pricing::cost_usd(&tokens, &price);
        unsafe { *out = usd };
        AgStatus::Ok
    })
}

/// Write the default on-disk cache directory (platform-specific) as a
/// library-owned C string to `*out`; free it with [`ag_string_free`].
///
/// # Safety
/// `out` is a valid writable `*mut *mut c_char`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_default_cache_dir(out: *mut *mut c_char) -> AgStatus {
    guard(|| {
        clear_last_error();
        if out.is_null() {
            set_last_error("out pointer is null");
            return AgStatus::InvalidArgument;
        }
        let dir = skopli_wire::parse::default_cache_dir();
        let c = match CString::new(dir) {
            Ok(c) => c,
            Err(e) => {
                set_last_error(format!("cache dir contains interior NUL: {e}"));
                return AgStatus::Internal;
            }
        };
        unsafe { *out = c.into_raw() };
        AgStatus::Ok
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
    guard(|| {
        clear_last_error();
        if out.is_null() {
            set_last_error("out pointer is null");
            return AgStatus::InvalidArgument;
        }
        let opts = match unsafe { json::read_json_opt(opts_json, opts_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        match pricing_opts::pricing_from_options(&opts) {
            Ok(pricing) => {
                let handle = Box::into_raw(Box::new(AgPricing { inner: pricing }));
                unsafe { *out = handle };
                AgStatus::Ok
            }
            Err(msg) => {
                set_last_error(msg);
                AgStatus::Catalog
            }
        }
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
    guard(|| {
        clear_last_error();
        let Some(handle) = (unsafe { pricing.as_ref() }) else {
            set_last_error("pricing handle is null");
            return AgStatus::InvalidArgument;
        };
        let events = match unsafe { read_events(events_json, events_len) } {
            Ok(e) => e,
            Err(s) => return s,
        };
        let opts = match unsafe { json::read_json_opt(opts_json, opts_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        match pipeline::price_events(&handle.inner, &events, &opts) {
            Ok(value) => write_json(out, value),
            Err(msg) => {
                set_last_error(msg);
                AgStatus::Catalog
            }
        }
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
    guard(|| {
        clear_last_error();
        let Some(handle) = (unsafe { pricing.as_ref() }) else {
            set_last_error("pricing handle is null");
            return AgStatus::InvalidArgument;
        };
        if model.is_null() || model_len == 0 {
            set_last_error("model name buffer is null or empty");
            return AgStatus::InvalidArgument;
        }
        let bytes = unsafe { std::slice::from_raw_parts(model as *const u8, model_len) };
        let name = match std::str::from_utf8(bytes) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                set_last_error("model name buffer is null or empty");
                return AgStatus::InvalidArgument;
            }
            Err(e) => {
                set_last_error(format!("model name is not valid UTF-8: {e}"));
                return AgStatus::InvalidArgument;
            }
        };
        let lookup = handle.inner.lookup_model(name);
        write_json(out, skopli_wire::wire::price_lookup_json(&lookup))
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
    guard(|| {
        clear_last_error();
        let Some(handle) = (unsafe { pricing.as_ref() }) else {
            set_last_error("pricing handle is null");
            return AgStatus::InvalidArgument;
        };
        let rollups = match unsafe { json::read_json_opt(rollups_json, rollups_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        match skopli_wire::pipeline::price_rollups(&handle.inner, &rollups) {
            Ok(value) => write_json(out, value),
            Err(msg) => {
                set_last_error(msg);
                AgStatus::Catalog
            }
        }
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
    guard(|| {
        clear_last_error();
        let Some(handle) = (unsafe { pricing.as_ref() }) else {
            set_last_error("pricing handle is null");
            return AgStatus::InvalidArgument;
        };
        let infos = handle.inner.catalog_infos();
        let value = Value::Array(infos.iter().map(wire::catalog_info_json).collect());
        write_json(out, value)
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

/// Run `body` producing a JSON value for a `(opts_json, out)` fallible fn,
/// clearing the error, parsing options, and writing the result buffer.
fn with_out(
    out: *mut AgBuf,
    opts_json: *const c_char,
    opts_len: usize,
    body: impl FnOnce(Value) -> Result<Value, AgStatus>,
) -> AgStatus {
    guard(|| {
        clear_last_error();
        let opts = match unsafe { json::read_json_opt(opts_json, opts_len) } {
            Ok(v) => v,
            Err(s) => return s,
        };
        match body(opts) {
            Ok(value) => write_json(out, value),
            Err(status) => status,
        }
    })
}

/// Serialize `value` to compact JSON and move it into `*out` as an owned
/// `AgBuf`. `InvalidArgument` when `out` is null.
fn write_json(out: *mut AgBuf, value: Value) -> AgStatus {
    if out.is_null() {
        set_last_error("out pointer is null");
        return AgStatus::InvalidArgument;
    }
    let text = match serde_json::to_string(&value) {
        Ok(t) => t,
        Err(e) => {
            set_last_error(format!("failed to serialize result: {e}"));
            return AgStatus::Internal;
        }
    };
    unsafe { *out = AgBuf::from_string(text) };
    AgStatus::Ok
}

/// Read and parse a `UsageEvent[]` from a JSON buffer. A null/empty buffer is an
/// empty slice. Parsing itself is shared with the other bindings via
/// [`skopli_wire::parse::parse_events`]; only the FFI buffer read and the
/// error-channel mapping are capi-specific.
unsafe fn read_events(ptr: *const c_char, len: usize) -> Result<Vec<UsageEvent>, AgStatus> {
    let value = unsafe { json::read_json_opt(ptr, len) }?;
    skopli_wire::parse::parse_events(&value).map_err(|msg| {
        set_last_error(msg);
        AgStatus::InvalidArgument
    })
}

//! PyO3 binding for `skopli-core`. This is the native engine (`_skopli`)
//! behind the typed `skopli` Python package: the pure-Python facade keeps
//! the idiomatic surface (snake_case functions, frozen dataclasses, `StrEnum`,
//! `PriceHit | PriceMiss` unions, the `SkopliError` hierarchy), and delegates
//! the pure data + math work to the functions here.
//!
//! Boundary shape: every call takes/returns JSON strings (`serde_json`), so the
//! Python side owns (de)serialization to its own types and the ABI stays a single
//! stable string-in/string-out contract - the same JSON-boundary design the C ABI
//! and node bindings use.
//!
//! GIL: sync-only v1. Every entry point releases the GIL via
//! [`Python::detach`] around the JSON parse + core work, so callers can
//! parallelise with `asyncio.to_thread` / threads. Only the final result-string
//! materialisation touches the interpreter.
//!
//! Errors: parse/domain failures raise a single native `_SkopliError` that
//! carries a `kind` string (`"invalid_argument"` | `"catalog"`); the Python
//! facade re-raises it into the typed `InvalidArgumentError` / `CatalogError`
//! hierarchy. Keeping one native exception leaves the Rust side idiom-free.

mod parse;
mod pipeline;
mod pricing_opts;
mod wire;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use serde_json::Value;

mod modules;

use modules::{ErrKind, PyResultExt};

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

create_exception!(_skopli, SkopliError, PyException);

// ---------------------------------------------------------------------------
// Module functions (all sync, all release the GIL)
// ---------------------------------------------------------------------------

/// The default on-disk pricing cache directory (platform-specific layout).
#[pyfunction]
fn default_cache_dir(py: Python<'_>) -> String {
    py.detach(parse::default_cache_dir)
}

/// Detect which harnesses have readable data under `{home?, env?}` (from
/// `options_json`). Returns a `{supported, unsupported}` JSON string.
#[pyfunction]
fn detect_harnesses(py: Python<'_>, options_json: &str) -> PyResult<String> {
    let opts = options_json.to_owned();
    py.detach(move || {
        let value = parse_json(&opts)?;
        to_json(&pipeline::detect(&value))
    })
    .into_py_err(py)
}

/// Read usage across the selected harnesses under `{home?, env?, harnesses?,
/// since?, until?, tz?, subagents?}` (from `options_json`). Returns an
/// `{events, diagnostics, skipped}` JSON string.
#[pyfunction]
fn read_usage(py: Python<'_>, options_json: &str) -> PyResult<String> {
    let opts = options_json.to_owned();
    py.detach(move || {
        let value = parse_json(&opts)?;
        let options = pipeline::parse_read_options(&value).map_err(DomainError::invalid)?;
        to_json(&pipeline::read_usage(&options))
    })
    .into_py_err(py)
}

/// Roll up an event array by a dimension. `events_json` is a `UsageEvent[]`;
/// `options_json` is `{by, tz?}`. Returns a `Rollup[]` JSON string.
#[pyfunction]
fn rollup(py: Python<'_>, events_json: &str, options_json: &str) -> PyResult<String> {
    let events_json = events_json.to_owned();
    let options_json = options_json.to_owned();
    py.detach(move || {
        let events =
            parse::parse_events(&parse_json(&events_json)?).map_err(DomainError::invalid)?;
        let options = parse_json(&options_json)?;
        let out = pipeline::rollup_events(&events, &options).map_err(DomainError::invalid)?;
        to_json(&out)
    })
    .into_py_err(py)
}

/// Compute the USD cost of a token bundle under a flat/tiered price.
/// `tokens_json` is a `TokenCounts`; `price_json` is a `ModelPrice`. Returns the
/// number.
#[pyfunction]
fn cost_usd(py: Python<'_>, tokens_json: &str, price_json: &str) -> PyResult<f64> {
    let tokens_json = tokens_json.to_owned();
    let price_json = price_json.to_owned();
    py.detach(move || {
        let tokens =
            parse::parse_tokens(&parse_json(&tokens_json)?).map_err(DomainError::invalid)?;
        let price = parse::parse_price(&parse_json(&price_json)?).map_err(DomainError::invalid)?;
        Ok(skopli_core::pricing::cost_usd(&tokens, &price))
    })
    .into_py_err(py)
}

// ---------------------------------------------------------------------------
// Pricing handle
// ---------------------------------------------------------------------------

/// A pricing engine over a fixed set of facade-loaded catalogs. Constructed from
/// `create_pricing` options (mode + overrides + pre-fetched catalogs); the facade
/// owns fetching / caching / refresh and rebuilds a new handle whenever its
/// catalogs change, so this holds an immutable snapshot. Holds only an in-memory
/// catalog set - GC frees it, no context manager needed.
#[pyclass(name = "Pricing", module = "skopli._skopli")]
struct PyPricing {
    inner: skopli_core::pricing::Pricing,
}

#[pymethods]
impl PyPricing {
    /// Build a pricing engine. `options_json` is `{mode?, overrides?, catalogs?}`.
    #[new]
    fn new(py: Python<'_>, options_json: &str) -> PyResult<Self> {
        let opts = options_json.to_owned();
        let pricing = py
            .detach(move || {
                let value = parse_json(&opts)?;
                pricing_opts::pricing_from_options(&value).map_err(DomainError::catalog)
            })
            .into_py_err(py)?;
        Ok(Self { inner: pricing })
    }

    /// Provenance of the loaded catalogs: `CatalogInfo[]` JSON.
    fn catalogs(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| {
            let infos: Vec<Value> = self
                .inner
                .catalog_infos()
                .iter()
                .map(wire::catalog_info_json)
                .collect();
            to_json(&Value::Array(infos))
        })
        .into_py_err(py)
    }

    /// Look up a model: a `PriceHit | PriceMiss` JSON.
    fn lookup_model(&self, py: Python<'_>, model: &str) -> PyResult<String> {
        let model = model.to_owned();
        py.detach(|| to_json(&wire::price_lookup_json(&self.inner.lookup_model(&model))))
            .into_py_err(py)
    }

    /// Price a set of rollups: `PricedRollup[]` JSON. `rollups_json` is a
    /// `Rollup[]`.
    fn price_rollups(&self, py: Python<'_>, rollups_json: &str) -> PyResult<String> {
        let rollups_json = rollups_json.to_owned();
        py.detach(|| {
            let rollups = parse_json(&rollups_json)?;
            let out =
                pipeline::price_rollups(&self.inner, &rollups).map_err(DomainError::catalog)?;
            to_json(&out)
        })
        .into_py_err(py)
    }

    /// Price a batch of events grouped by a rollup dimension:
    /// `PricedEventGroup[]` JSON. `events_json` is a `UsageEvent[]`;
    /// `options_json` is `{by, tz?}`.
    fn price_events(
        &self,
        py: Python<'_>,
        events_json: &str,
        options_json: &str,
    ) -> PyResult<String> {
        let events_json = events_json.to_owned();
        let options_json = options_json.to_owned();
        py.detach(|| {
            let events =
                parse::parse_events(&parse_json(&events_json)?).map_err(DomainError::invalid)?;
            let options = parse_json(&options_json)?;
            let out = pipeline::price_events(&self.inner, &events, &options)
                .map_err(DomainError::catalog)?;
            to_json(&out)
        })
        .into_py_err(py)
    }
}

/// The native extension module `_skopli`.
#[pymodule]
fn _skopli(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("SkopliError", m.py().get_type::<SkopliError>())?;
    m.add_function(wrap_pyfunction!(default_cache_dir, m)?)?;
    m.add_function(wrap_pyfunction!(detect_harnesses, m)?)?;
    m.add_function(wrap_pyfunction!(read_usage, m)?)?;
    m.add_function(wrap_pyfunction!(rollup, m)?)?;
    m.add_function(wrap_pyfunction!(cost_usd, m)?)?;
    m.add_class::<PyPricing>()?;
    Ok(())
}

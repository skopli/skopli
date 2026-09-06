//! Build a core [`Pricing`] instance from the `ag_pricing_new` options JSON.
//!
//! The C ABI has no callback surface: a caller supplies pricing either as
//! programmatic `overrides`, as
//! pre-fetched `catalogs` (each a source name + optional fetchedAt + a prices
//! map already in the core catalog grammar, OR a raw source payload plus its
//! `format`), and/or by leaving the built-in sources ON. For the C ABI the
//! built-in path IS the seam: `builtin_sources`
//! defaults TRUE here, and the core fetches openrouter/litellm/models.dev with a
//! disk cache. `offline` and `max_cache_age_ms` steer that built-in fetch/cache;
//! explicit `overrides`/`catalogs` still win and merge ahead of the built-ins per
//! the spec priority order. The richer facades (node/py/ruby) fetch facade-side
//! and pass `builtin_sources: false` to opt out.
//!
//! The `overrides`/`catalogs` parsing itself is shared with the other bindings
//! via [`skopli_wire::pricing_opts::parse_pricing_parts`]; only the built-in
//! fetch seam (which no other binding has) lives here.

use serde_json::Value;
use skopli_core::pricing::Pricing;
#[cfg(feature = "net")]
pub use skopli_core::pricing::sources::SourceUrls;
#[cfg(feature = "net")]
use skopli_core::pricing::sources::{BuiltinOptions, load_builtin_catalogs_with_urls};
use skopli_wire::pricing_opts::parse_pricing_parts;

/// The error returned when a caller asks for built-in sources but this build was
/// compiled without the `net` feature (the network-free default `libskopli`).
#[cfg_attr(feature = "net", allow(dead_code))]
pub const NO_BUILTIN_SOURCES: &str = "builtin pricing sources are unavailable: this build of skopli was compiled without the \"net\" feature";

/// Parse the `ag_pricing_new` options into a [`Pricing`] instance, or an error
/// message (turned into `Catalog` status by the caller). Production entry point:
/// the built-in fetch always targets the live source endpoints.
pub(crate) fn pricing_from_options(opts: &Value) -> Result<Pricing, String> {
    #[cfg(feature = "net")]
    {
        pricing_from_options_with_urls(opts, &SourceUrls::default())
    }
    #[cfg(not(feature = "net"))]
    {
        pricing_from_options_inner(opts)
    }
}

/// Shared parse of the `overrides`/`catalogs`/`builtin_sources` options into a
/// [`Pricing`], with the built-in fetch driven by `urls`. `urls` is `None` in
/// non-`net` builds (where the built-in path errors) and the live defaults in
/// production; the net-gated fetch-proving test passes a local server.
#[cfg(feature = "net")]
#[doc(hidden)]
pub fn pricing_from_options_with_urls(opts: &Value, urls: &SourceUrls) -> Result<Pricing, String> {
    let (mut catalogs, override_index, mode) = parse_pricing_parts(opts)?;

    // Built-in sources are the C ABI's seam. Default ON; opt out with
    // `builtin_sources: false`. They sit at the LOWEST priority (after any
    // explicit overrides/catalogs), matching the spec's "explicit catalogs still
    // win/merge" rule.
    let builtin_sources = opts
        .get("builtinSources")
        .or_else(|| opts.get("builtin_sources"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if builtin_sources {
        let offline = opts
            .get("offline")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_cache_age_ms = opts
            .get("maxCacheAgeMs")
            .or_else(|| opts.get("max_cache_age_ms"))
            .and_then(Value::as_u64);
        let refresh = opts
            .get("refresh")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let cache_dir = opts
            .get("cacheDir")
            .or_else(|| opts.get("cache_dir"))
            .and_then(Value::as_str)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(skopli_core::paths::default_cache_dir()));
        let builtin = BuiltinOptions {
            cache_dir,
            offline,
            max_cache_age_ms,
        };
        catalogs.extend(load_builtin_catalogs_with_urls(&builtin, refresh, urls));
    }

    Ok(Pricing::new(catalogs, override_index, mode))
}

/// Non-`net` parse: the built-in path is unavailable, so a caller asking for it
/// gets [`NO_BUILTIN_SOURCES`]; otherwise the explicit parts build the instance.
#[cfg(not(feature = "net"))]
fn pricing_from_options_inner(opts: &Value) -> Result<Pricing, String> {
    let (catalogs, override_index, mode) = parse_pricing_parts(opts)?;

    let builtin_sources = opts
        .get("builtinSources")
        .or_else(|| opts.get("builtin_sources"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if builtin_sources {
        return Err(NO_BUILTIN_SOURCES.to_owned());
    }

    Ok(Pricing::new(catalogs, override_index, mode))
}

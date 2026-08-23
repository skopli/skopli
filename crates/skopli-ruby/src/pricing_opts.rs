//! Build a core `Pricing` from the `create_pricing` options JSON for the Ruby
//! binding. The implementation is shared across the direct-core bindings whose
//! facade owns fetching + cache and lives in `skopli-wire`; this module
//! re-exports it so `crate::pricing_opts::*` call sites stay unchanged.

pub(crate) use skopli_wire::pricing_opts::pricing_from_options;

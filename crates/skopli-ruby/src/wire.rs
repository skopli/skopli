//! Wire-shape serializers for the Ruby binding. The implementation is shared
//! across the direct-core bindings and lives in `skopli-wire`; this module
//! re-exports the entries the binding uses so `crate::wire::*` call sites stay
//! unchanged.

pub(crate) use skopli_wire::wire::{catalog_info_json, price_lookup_json};

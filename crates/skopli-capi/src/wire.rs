//! Wire-shape serializers for the pricing value types the core exposes with
//! non-`Serialize` idiomatic shapes. Re-exported verbatim from
//! `skopli-wire`: the C ABI emits the SAME JSON shapes as every other
//! binding, so there is a single source of truth (this also fixes the historical
//! drift where the capi copy dropped `tierMode` from `model_price_json`).

pub(crate) use skopli_wire::wire::catalog_info_json;

//! The JSON wire shared by every binding over `skopli-core`: `skopli-node`,
//! `skopli-py`, `skopli-ruby` and `skopli-capi`. One JSON shape, one serializer
//! and one deserializer per type; a binding adds only its host-language
//! marshalling (JS strings, Python exceptions, the Ruby GVL, C pointers).
//!
//! - [`parse`]: the `{home, env}` path-override context and the camelCase
//!   `UsageEvent[]` / `TokenCounts` / `Rollup` / `ModelPrice` input shapes.
//! - [`wire`]: serializers for the pricing value types the core exposes with
//!   idiomatic (non-`Serialize`) shapes, mirroring the TS JSON shapes exactly.
//! - [`pipeline`]: `rollup_events`, `price_events`, `price_rollups`.
//! - [`read`]: the `read_usage` and per-harness `read_harness` envelopes plus
//!   harness detection.
//! - [`pricing_opts`]: the `create_pricing` option grammar.
//!
//! Nothing here touches the network or the C ABI: the whole surface is
//! `serde_json::Value` over `skopli-core`.

pub mod parse;
pub mod pipeline;
pub mod pricing_opts;
pub mod read;
pub mod wire;

//! Workspace-internal helpers shared by every direct-core binding
//! (`skopli-node`, `skopli-py`, `skopli-ruby`) and, for the parse
//! helpers, `skopli-capi`. These were previously duplicated verbatim in each
//! binding crate:
//!
//! - [`parse`] — JSON-boundary parsing of the `{home, env}` path-override
//!   context and the camelCase `UsageEvent[]` / `TokenCounts` / `ModelPrice`
//!   wire shapes the facades send.
//! - [`wire`] — serializers for the pricing value types the core exposes with
//!   idiomatic (non-`Serialize`) shapes, mirroring the TS JSON shapes exactly.
//! - [`pipeline`] — the event-group / rollup pricing that is identical across
//!   the bindings (`rollup_events`, `price_events`, `price_rollups`).
//! - [`read`] — the multi-reader `read_usage` envelope + harness detection
//!   shared by the bindings that own the whole read+filter loop (py, ruby).
//! - [`pricing_opts`] — `create_pricing` option parsing shared by the bindings
//!   whose facade owns fetching + cache (py, ruby).
//!
//! Nothing here touches the network or the C ABI: the whole surface is
//! `serde_json::Value` over `skopli-core`. Binding-specific concerns
//! (node's per-harness raw gather, the capi FFI types, node's fetch-free
//! `create_pricing` shape) stay in each binding crate.

pub mod parse;
pub mod pipeline;
pub mod pricing_opts;
pub mod read;
pub mod wire;

//! Pipeline orchestration for the Python binding. Every function here is shared
//! across the direct-core bindings that own the whole read+filter loop and lives
//! in `skopli-wire`; this module re-exports them so `crate::pipeline::*` call
//! sites stay unchanged. The rollup / event-group pricing comes from
//! `skopli-wire::pipeline`; the `read_usage` envelope + detection from
//! `skopli-wire::read`.

pub(crate) use skopli_wire::pipeline::{price_events, price_rollups, rollup_events};
pub(crate) use skopli_wire::read::{detect, parse_read_options, read_usage};

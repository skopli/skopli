//! Pipeline orchestration over `skopli-core`, shared verbatim with the other
//! direct-core bindings via `skopli-wire`. The multi-reader `read_usage`
//! envelope + harness detection come from [`skopli_wire::read`]; the rollup
//! and event-group pricing come from [`skopli_wire::pipeline`] (which now
//! delegates all pricing math to the gold-tested typed core engine). Nothing
//! here touches FFI types — the C ABI shell in `lib.rs` owns that boundary.

pub(crate) use skopli_wire::pipeline::{price_events, rollup_events};
pub(crate) use skopli_wire::read::{detect, parse_read_options, read_usage};

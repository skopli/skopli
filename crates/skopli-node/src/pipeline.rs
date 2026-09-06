//! Pipeline orchestration over `skopli-core` for the node binding: the
//! per-harness `read_harness` gather and the supported-harness set. The rollup
//! and event-group / rollup pricing are identical across the direct-core
//! bindings and live in `skopli-wire::pipeline`, re-exported below so
//! `pipeline::*` call sites stay unchanged.
//!
//! Seam split: the TS facade owns the date-bounds resolution (IANA-tz / DST via `Intl`), the
//! timestamp-drop / since / until / subagent FILTER LOOP, and harness detection.
//! The addon is a pure data hook: given `{home, env}` and a single harness id,
//! it returns that harness's RAW events plus warnings and skipped paths. The
//! facade then merges the addon's harnesses with any TS-only harness (e.g.
//! `kilo`, not yet ported to the core) and runs the one shared filter loop over
//! the union, so live TS and live Rust agree at the API boundary and no
//! filtering logic is duplicated across the seam.
//!
//! Diagnostic (warning) messages are emitted by the readers already relativized
//! (the readers route through the ambient warn sink); the addon only strips the
//! single trailing newline the TS `readUsage` strips.

use serde_json::{Value, json};
use skopli_core::readers::reader::ReaderContext;

pub(crate) use skopli_wire::pipeline::{price_events, price_rollups, rollup_events};

/// The set of harness ids the core has a registered reader for. The facade uses
/// this to partition its resolved harness list into addon-served vs TS-served
/// (the TS reader layer still handles harnesses not yet ported to the core,
/// so both coexist until cutover).
pub(crate) fn supported_harnesses() -> Vec<String> {
    skopli_core::readers::registered_readers()
        .iter()
        .map(|r| r.harness_id().to_owned())
        .collect()
}

/// Gather ONE harness's raw usage under `{home, env}`: its events (unfiltered),
/// its warnings as diagnostics, and its skipped paths. Returns
/// `{events, diagnostics, skipped}` where `diagnostics` is a `Diagnostic[]` and
/// `skipped` is a `string[]` (the facade keys it by harness). Unknown harnesses
/// (no registered reader) yield an all-empty envelope so the facade can route
/// them to the TS reader layer instead.
pub(crate) fn read_harness(harness: &str, opts: &Value) -> Value {
    let ctx: ReaderContext = crate::parse::context_from_options(opts);

    let mut diagnostics: Vec<Value> = Vec::new();

    let Some(reader) = skopli_core::readers::registered_reader(harness) else {
        return json!({ "events": [], "diagnostics": [], "skipped": [] });
    };
    let result = reader.read(&ctx);
    let harness_id = reader.harness_id();

    for warning in &result.warnings {
        // Match the TS `text.replace(/\n$/, "")`: strip exactly ONE trailing
        // newline, not a run of them.
        let message = warning
            .message
            .strip_suffix('\n')
            .unwrap_or(&warning.message);
        diagnostics.push(json!({
            "severity": "warning",
            "message": message,
            "harness": harness_id,
        }));
    }

    let events_json: Vec<Value> = result
        .events
        .iter()
        .map(|e| serde_json::to_value(e).expect("serialize event"))
        .collect();

    json!({
        "events": events_json,
        "diagnostics": diagnostics,
        "skipped": result.skipped,
    })
}

//! ABI smoke test: drive the `extern "C"` surface exactly as a C consumer
//! would - JSON in, JSON out, `AgStatus` codes, `AgBuf`/string ownership,
//! thread-local error message - and assert the round-trip
//! `ag_read_usage -> ag_rollup -> ag_pricing_price_events` matches the core's
//! native output for the claude/basic + pricing/basic golden cases.
//!
//! Everything here goes through the public FFI functions (the crate is also an
//! rlib so the test can link them); no core internals are called except to build
//! the expected values the ABI output is compared against.

use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use skopli::{
    AgBuf, AgStatus, ag_abi_version, ag_buf_free, ag_cost_usd, ag_default_cache_dir,
    ag_detect_harnesses, ag_last_error_message, ag_pricing_catalog_info, ag_pricing_free,
    ag_pricing_lookup_model, ag_pricing_new, ag_pricing_price_events, ag_pricing_price_rollups,
    ag_read_usage, ag_rollup, ag_schema_version, ag_string_free, ag_version,
};

// ---------------------------------------------------------------------------
// Thin safe wrappers around the raw ABI
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

/// Take ownership of an out-`AgBuf`, copy it to a `Value`, and free it.
fn buf_to_value(buf: AgBuf) -> Value {
    let bytes = if buf.ptr.is_null() || buf.len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(buf.ptr, buf.len).to_vec() }
    };
    unsafe { ag_buf_free(buf) };
    serde_json::from_slice(&bytes).expect("valid JSON out")
}

/// A JSON value as a NUL-terminated C string (owned; kept alive by the caller).
fn cjson(value: &Value) -> CString {
    CString::new(serde_json::to_string(value).unwrap()).unwrap()
}

fn last_error() -> Option<String> {
    let mut out: *mut c_char = std::ptr::null_mut();
    let status = unsafe { ag_last_error_message(&mut out) };
    assert_eq!(status, AgStatus::Ok);
    if out.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { ag_string_free(out) };
    Some(s)
}

/// A distinct message no negative-path branch produces; seeded before each case
/// so a stale reuse of a prior message is caught.
const SENTINEL: &str = "invalid date: __sentinel__";

/// Set the thread-local error to [`SENTINEL`] via a known ABI failure, so the
/// next call must overwrite it to pass.
fn seed_sentinel_error() {
    let opts = cjson(&json!({ "since": "__sentinel__" }));
    let mut out = AgBuf {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let status = unsafe { ag_read_usage(opts.as_ptr(), opts.as_bytes().len(), &mut out) };
    assert_eq!(status, AgStatus::InvalidArgument);
    assert_eq!(last_error().as_deref(), Some(SENTINEL));
}

/// Call `ag_read_usage` with an options value, returning the parsed envelope.
fn read_usage(opts: &Value) -> Value {
    let c = cjson(opts);
    let mut out = AgBuf {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let status = unsafe { ag_read_usage(c.as_ptr(), c.as_bytes().len(), &mut out) };
    assert_eq!(
        status,
        AgStatus::Ok,
        "read_usage failed: {:?}",
        last_error()
    );
    buf_to_value(out)
}

fn rollup(events: &Value, opts: &Value) -> Value {
    let ce = cjson(events);
    let co = cjson(opts);
    let mut out = AgBuf {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let status = unsafe {
        ag_rollup(
            ce.as_ptr(),
            ce.as_bytes().len(),
            co.as_ptr(),
            co.as_bytes().len(),
            &mut out,
        )
    };
    assert_eq!(status, AgStatus::Ok, "rollup failed: {:?}", last_error());
    buf_to_value(out)
}

// ---------------------------------------------------------------------------
// Meta / lifecycle
// ---------------------------------------------------------------------------

#[test]
fn meta_functions() {
    assert_eq!(ag_abi_version(), 1);
    assert_eq!(ag_schema_version(), 1);
    let v = unsafe { CStr::from_ptr(ag_version()) }
        .to_string_lossy()
        .into_owned();
    assert!(!v.is_empty(), "version string non-empty");
}

#[test]
fn default_cache_dir_is_nonempty_and_freeable() {
    let mut out: *mut c_char = std::ptr::null_mut();
    let status = unsafe { ag_default_cache_dir(&mut out) };
    assert_eq!(status, AgStatus::Ok);
    assert!(!out.is_null());
    let dir = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    assert!(dir.contains("skopli"), "cache dir mentions skopli: {dir}");
    unsafe { ag_string_free(out) };
}

// ---------------------------------------------------------------------------
// Error channel + argument validation
// ---------------------------------------------------------------------------

#[test]
fn null_out_is_invalid_argument() {
    let opts = cjson(&json!({}));
    let status =
        unsafe { ag_read_usage(opts.as_ptr(), opts.as_bytes().len(), std::ptr::null_mut()) };
    assert_eq!(status, AgStatus::InvalidArgument);
    assert!(last_error().is_some());
}

#[test]
fn malformed_json_is_invalid_argument() {
    let bad = CString::new("{ not json").unwrap();
    let mut out = AgBuf {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let status = unsafe { ag_read_usage(bad.as_ptr(), bad.as_bytes().len(), &mut out) };
    assert_eq!(status, AgStatus::InvalidArgument);
    let msg = last_error().expect("error message set");
    assert!(msg.contains("invalid JSON"), "message: {msg}");
}

#[test]
fn invalid_since_is_invalid_argument() {
    let opts = cjson(&json!({ "since": "not-a-date" }));
    let mut out = AgBuf {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let status = unsafe { ag_read_usage(opts.as_ptr(), opts.as_bytes().len(), &mut out) };
    assert_eq!(status, AgStatus::InvalidArgument);
    assert!(last_error().unwrap().contains("invalid date"));
}

// ---------------------------------------------------------------------------
// cost_usd
// ---------------------------------------------------------------------------

#[test]
fn cost_usd_flat_matches_hand_math() {
    let tokens = cjson(&json!({
        "input": 1000, "output": 500, "cacheRead": 0, "cacheWrite": 0, "reasoning": 0
    }));
    let price = cjson(&json!({ "input": 1.25, "output": 10.0 }));
    let mut out: f64 = -1.0;
    let status = unsafe {
        ag_cost_usd(
            tokens.as_ptr(),
            tokens.as_bytes().len(),
            price.as_ptr(),
            price.as_bytes().len(),
            &mut out,
        )
    };
    assert_eq!(status, AgStatus::Ok, "cost_usd failed: {:?}", last_error());
    let expected = (1000.0 * 1.25 + 500.0 * 10.0) / 1_000_000.0;
    assert_eq!(out, expected);
}

// ---------------------------------------------------------------------------
// Round-trip: detect / read / rollup over the claude/basic golden case
// ---------------------------------------------------------------------------

fn claude_options() -> Value {
    let input_dir = repo_root()
        .join("golden")
        .join("claude")
        .join("basic")
        .join("input");
    json!({
        "home": "/nonexistent",
        "env": { "CLAUDE_CONFIG_DIR": input_dir.to_string_lossy() },
        "harnesses": ["claude"],
        "tz": "UTC",
    })
}

#[test]
fn detect_finds_claude() {
    let detection = {
        let c = cjson(&claude_options());
        let mut out = AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status = unsafe { ag_detect_harnesses(c.as_ptr(), c.as_bytes().len(), &mut out) };
        assert_eq!(status, AgStatus::Ok, "detect failed: {:?}", last_error());
        buf_to_value(out)
    };
    let supported = detection["supported"].as_array().unwrap();
    assert!(
        supported.iter().any(|v| v == "claude"),
        "claude detected: {detection}"
    );
    assert!(detection["unsupported"].as_array().unwrap().is_empty());
}

#[test]
fn read_rollup_roundtrip_matches_core_gold() {
    let envelope = read_usage(&claude_options());
    let events = &envelope["events"];
    assert!(!events.as_array().unwrap().is_empty(), "read some events");

    // Sort events the way the exporter/gold does before rolling up.
    let mut evs: Vec<Value> = events.as_array().unwrap().clone();
    evs.sort_by(|a, b| {
        let k = |v: &Value| {
            (
                v["timestamp"].as_str().unwrap_or("").to_owned(),
                v["sessionId"].as_str().unwrap_or("").to_owned(),
                v["messageId"].as_str().unwrap_or("").to_owned(),
                v["model"].as_str().unwrap_or("").to_owned(),
            )
        };
        k(a).cmp(&k(b))
    });
    let sorted_events = Value::Array(evs);

    // Roll up by model through the ABI, compare to the committed gold's model
    // lane. The gold rollup is over the same reader output the core produces, so
    // an ABI mismatch means the JSON boundary corrupted something.
    let by_model = rollup(&sorted_events, &json!({ "by": "model", "tz": "UTC" }));
    let gold: Value = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root()
                .join("golden")
                .join("claude")
                .join("basic")
                .join("expected-rollup.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        by_model, gold["by"]["model"],
        "ABI rollup(by=model) must equal the core gold model lane"
    );
}

/// Parity for the tz-omitted date-only filtering default across the ABI: an
/// omitted `tz` must resolve the SAME zone the system reports, so a date-only
/// `since`/`until` read with no `tz` selects exactly the events a read that
/// names the system zone selects. Portable: it asserts equality between the two
/// ABI calls rather than pinning any host zone.
#[test]
fn read_omitted_tz_matches_explicit_system_zone() {
    let input_dir = repo_root()
        .join("golden")
        .join("claude")
        .join("basic")
        .join("input");
    let stamps = |opts: &Value| -> Vec<String> {
        let env = read_usage(opts);
        let mut s: Vec<String> = env["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["timestamp"].as_str().unwrap_or("").to_owned())
            .collect();
        s.sort();
        s
    };
    let base = json!({
        "home": "/nonexistent",
        "env": { "CLAUDE_CONFIG_DIR": input_dir.to_string_lossy() },
        "harnesses": ["claude"],
        "since": "2026-08-01",
        "until": "2026-08-01",
    });
    let omitted = stamps(&base);

    let system = jiff::tz::TimeZone::system();
    let name = system.iana_name().unwrap_or("UTC");
    let mut explicit_opts = base.clone();
    explicit_opts["tz"] = json!(name);
    let explicit = stamps(&explicit_opts);

    assert_eq!(
        omitted, explicit,
        "omitted-tz filtering must equal explicit system-zone filtering over the ABI"
    );
}

// ---------------------------------------------------------------------------
// Pricing round-trip: price_events over the pricing/basic golden catalogs
// ---------------------------------------------------------------------------

fn read_catalog_payload(name: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            repo_root()
                .join("golden")
                .join("pricing")
                .join("catalogs")
                .join(format!("{name}.json")),
        )
        .unwrap(),
    )
    .unwrap()
}

/// The exact synthetic events from the pricing conformance case.
fn pricing_events() -> Value {
    let ev = |mid: &str, sec: u32, model: &str, tokens: Value| {
        json!({
            "harness": "opencode",
            "timestamp": format!("2026-08-01T00:0{sec}:00.000Z"),
            "sessionId": "s",
            "messageId": mid,
            "turn": true,
            "subagent": false,
            "model": model,
            "tokens": tokens,
        })
    };
    let tk = |input, output, cr, cw, cw1h: Option<u64>, reasoning| {
        let mut o = json!({
            "input": input, "output": output, "cacheRead": cr,
            "cacheWrite": cw, "reasoning": reasoning
        });
        if let Some(v) = cw1h {
            o["cacheWrite1h"] = json!(v);
        }
        o
    };
    json!([
        ev("flat", 0, "gpt-5", tk(1000, 500, 0, 0, None, 0)),
        ev(
            "tiered",
            1,
            "claude-sonnet-4-5",
            tk(200_000, 10_000, 20_000, 30_000, Some(10_000), 2_000)
        ),
        ev(
            "base-1h",
            2,
            "claude-sonnet-4-5",
            tk(5_000, 1_000, 2_000, 4_000, Some(1_500), 0)
        ),
        ev(
            "alias",
            3,
            "us.anthropic.claude-opus-4-6-20260115-v1:0",
            tk(800, 200, 0, 0, None, 0)
        ),
        ev(
            "miss",
            4,
            "totally-unknown-model-9000",
            tk(100, 100, 0, 0, None, 0)
        ),
    ])
}

#[test]
fn price_events_and_catalog_info_roundtrip() {
    const PINNED: &str = "2026-08-01T00:00:00.000Z";
    let opts = json!({
        "mode": "calculate",
        // Hermetic: explicit catalogs only, built-in network sources OFF.
        "builtinSources": false,
        "catalogs": [
            { "source": "openrouter", "fetchedAt": PINNED, "format": "openrouter",
              "payload": read_catalog_payload("openrouter") },
            { "source": "litellm", "fetchedAt": PINNED, "format": "litellm",
              "payload": read_catalog_payload("litellm") },
        ],
    });

    let mut handle = std::ptr::null_mut();
    let c = cjson(&opts);
    let status = unsafe { ag_pricing_new(c.as_ptr(), c.as_bytes().len(), &mut handle) };
    assert_eq!(
        status,
        AgStatus::Ok,
        "pricing_new failed: {:?}",
        last_error()
    );
    assert!(!handle.is_null());

    // catalog_info reports both sources with the right model counts.
    let infos = {
        let mut out = AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status = unsafe { ag_pricing_catalog_info(handle, &mut out) };
        assert_eq!(status, AgStatus::Ok);
        buf_to_value(out)
    };
    let infos = infos.as_array().unwrap();
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0]["source"], "openrouter");
    assert_eq!(infos[1]["source"], "litellm");
    assert!(infos[0]["models"].as_u64().unwrap() > 0);

    // price_events by model.
    let events = pricing_events();
    let ce = cjson(&events);
    let po = cjson(&json!({ "by": "model" }));
    let priced = {
        let mut out = AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status = unsafe {
            ag_pricing_price_events(
                handle,
                ce.as_ptr(),
                ce.as_bytes().len(),
                po.as_ptr(),
                po.as_bytes().len(),
                &mut out,
            )
        };
        assert_eq!(
            status,
            AgStatus::Ok,
            "price_events failed: {:?}",
            last_error()
        );
        buf_to_value(out)
    };

    unsafe { ag_pricing_free(handle) };

    let groups = priced.as_array().unwrap();
    assert!(!groups.is_empty());

    // The gpt-5 group is a flat priced hit; the unknown model is a miss. Verify
    // the shape and that a known model priced non-zero.
    let gpt5 = groups
        .iter()
        .find(|g| g["key"] == "gpt-5")
        .expect("gpt-5 group present");
    assert_eq!(gpt5["pricing"]["priced"], json!(true));
    assert!(gpt5["pricing"]["usd"].as_f64().unwrap() > 0.0);

    let miss = groups
        .iter()
        .find(|g| g["key"] == "totally-unknown-model-9000")
        .expect("miss group present");
    assert_eq!(miss["pricing"]["priced"], json!(false));
}

/// Build the same hermetic handle as `price_events_and_catalog_info_roundtrip`:
/// explicit openrouter + litellm catalogs, built-in network sources OFF.
fn hermetic_handle() -> *mut skopli::AgPricing {
    const PINNED: &str = "2026-08-01T00:00:00.000Z";
    let opts = json!({
        "mode": "calculate",
        "builtinSources": false,
        "catalogs": [
            { "source": "openrouter", "fetchedAt": PINNED, "format": "openrouter",
              "payload": read_catalog_payload("openrouter") },
            { "source": "litellm", "fetchedAt": PINNED, "format": "litellm",
              "payload": read_catalog_payload("litellm") },
        ],
    });
    let mut handle = std::ptr::null_mut();
    let c = cjson(&opts);
    let status = unsafe { ag_pricing_new(c.as_ptr(), c.as_bytes().len(), &mut handle) };
    assert_eq!(
        status,
        AgStatus::Ok,
        "pricing_new failed: {:?}",
        last_error()
    );
    assert!(!handle.is_null());
    handle
}

#[test]
fn price_rollups_matches_full_gold() {
    let gold: Value = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root()
                .join("golden")
                .join("pricing")
                .join("basic")
                .join("expected-priced-rollup.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let expected = gold["rollups"].as_array().unwrap();

    // Feed the pricer the gold's rollups minus their `pricing` field.
    let input: Vec<Value> = expected
        .iter()
        .map(|r| {
            let mut r = r.clone();
            r.as_object_mut().unwrap().remove("pricing");
            r
        })
        .collect();
    let input = Value::Array(input);

    let handle = hermetic_handle();
    let cr = cjson(&input);
    let priced = {
        let mut out = AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status =
            unsafe { ag_pricing_price_rollups(handle, cr.as_ptr(), cr.as_bytes().len(), &mut out) };
        assert_eq!(
            status,
            AgStatus::Ok,
            "price_rollups failed: {:?}",
            last_error()
        );
        buf_to_value(out)
    };
    unsafe { ag_pricing_free(handle) };

    let got = priced.as_array().unwrap();
    assert_eq!(got.len(), expected.len(), "same number of priced rollups");
    // FULL structural equality against the normative wire gold, no field
    // removal: every gold field (usd, tieredAggregate, source, key, the full
    // `price` object, and the miss shape) must match exactly.
    for (g, e) in got.iter().zip(expected.iter()) {
        assert_eq!(g, e, "priced rollup element mismatch");
    }
}

#[test]
fn lookup_model_hit_and_miss() {
    let handle = hermetic_handle();

    let lookup = |name: &str| -> Value {
        let bytes = name.as_bytes();
        let mut out = AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status = unsafe {
            ag_pricing_lookup_model(
                handle,
                bytes.as_ptr() as *const c_char,
                bytes.len(),
                &mut out,
            )
        };
        assert_eq!(status, AgStatus::Ok, "lookup failed: {:?}", last_error());
        buf_to_value(out)
    };

    let hit = lookup("gpt-5");
    assert_eq!(hit["priced"], json!(true));
    assert_eq!(hit["source"], json!("litellm"));

    let miss = lookup("totally-unknown-model-9000");
    assert_eq!(miss["priced"], json!(false));

    unsafe { ag_pricing_free(handle) };
}

/// Negative-path coverage for `ag_pricing_lookup_model`: each case asserts the
/// expected `AgStatus` and the exact stable error message from `lib.rs`. The one
/// message with a dynamic suffix ("invalid utf8") is checked by seeding a
/// sentinel error first and asserting the returned message both differs from the
/// sentinel (proving it is fresh) and carries the stable prefix.
#[test]
fn lookup_model_negative_paths() {
    let handle = hermetic_handle();
    let mut out = AgBuf {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let valid = b"gpt-5";
    let bad_utf8: &[u8] = &[0xff, 0xfe];
    // (label, handle, model ptr, model len, out ptr, expected message)
    let cases: &[(
        &str,
        *const skopli::AgPricing,
        *const c_char,
        usize,
        *mut AgBuf,
        &str,
    )] = &[
        (
            "null handle",
            std::ptr::null(),
            valid.as_ptr() as *const c_char,
            valid.len(),
            &mut out,
            "pricing handle is null",
        ),
        (
            "null out",
            handle,
            valid.as_ptr() as *const c_char,
            valid.len(),
            std::ptr::null_mut(),
            "out pointer is null",
        ),
        (
            "null model",
            handle,
            std::ptr::null(),
            4,
            &mut out,
            "model name buffer is null or empty",
        ),
        (
            "empty model",
            handle,
            valid.as_ptr() as *const c_char,
            0,
            &mut out,
            "model name buffer is null or empty",
        ),
        // Dynamic suffix: checked against the sentinel + prefix below.
        (
            "invalid utf8",
            handle,
            bad_utf8.as_ptr() as *const c_char,
            bad_utf8.len(),
            &mut out,
            "model name is not valid UTF-8: ",
        ),
    ];
    for (label, h, mptr, mlen, optr, want) in cases {
        seed_sentinel_error();
        let status = unsafe { ag_pricing_lookup_model(*h, *mptr, *mlen, *optr) };
        assert_eq!(status, AgStatus::InvalidArgument, "{label}");
        let msg = last_error().unwrap_or_default();
        assert_ne!(msg, SENTINEL, "{label}: message must be freshly set");
        if *label == "invalid utf8" {
            assert!(msg.starts_with(want), "{label}: message {msg:?}");
        } else {
            assert_eq!(msg, *want, "{label}");
        }
    }
    unsafe { ag_pricing_free(handle) };
}

/// Negative-path coverage for `ag_pricing_price_rollups`: null handle/out and
/// bad-buffer cases are `InvalidArgument`; malformed/non-array rollup bodies map
/// to `Catalog`. Each asserts the exact stable message from `lib.rs`; the one
/// dynamic-suffix message ("malformed json") is checked against a seeded
/// sentinel plus its stable prefix.
#[test]
fn price_rollups_negative_paths() {
    let handle = hermetic_handle();
    let mut out = AgBuf {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let valid = CString::new("[]").unwrap();
    let malformed = CString::new("{ not json").unwrap();
    let non_array = CString::new("{}").unwrap();
    // (label, handle, body ptr, body len, out ptr, expected status, expected message)
    let cases: &[(
        &str,
        *const skopli::AgPricing,
        *const c_char,
        usize,
        *mut AgBuf,
        AgStatus,
        &str,
    )] = &[
        (
            "null handle",
            std::ptr::null(),
            valid.as_ptr(),
            valid.as_bytes().len(),
            &mut out,
            AgStatus::InvalidArgument,
            "pricing handle is null",
        ),
        (
            "null out",
            handle,
            valid.as_ptr(),
            valid.as_bytes().len(),
            std::ptr::null_mut(),
            AgStatus::InvalidArgument,
            "out pointer is null",
        ),
        (
            "null body",
            handle,
            std::ptr::null(),
            0,
            &mut out,
            AgStatus::Catalog,
            "rollups must be a JSON array",
        ),
        // Dynamic suffix: checked against the sentinel + prefix below.
        (
            "malformed json",
            handle,
            malformed.as_ptr(),
            malformed.as_bytes().len(),
            &mut out,
            AgStatus::InvalidArgument,
            "invalid JSON: ",
        ),
        (
            "non-array body",
            handle,
            non_array.as_ptr(),
            non_array.as_bytes().len(),
            &mut out,
            AgStatus::Catalog,
            "rollups must be a JSON array",
        ),
    ];
    for (label, h, bptr, blen, optr, want, msg_want) in cases {
        seed_sentinel_error();
        let status = unsafe { ag_pricing_price_rollups(*h, *bptr, *blen, *optr) };
        assert_eq!(status, *want, "{label}");
        let msg = last_error().unwrap_or_default();
        assert_ne!(msg, SENTINEL, "{label}: message must be freshly set");
        if *label == "malformed json" {
            assert!(msg.starts_with(msg_want), "{label}: message {msg:?}");
        } else {
            assert_eq!(msg, *msg_want, "{label}");
        }
    }
    unsafe { ag_pricing_free(handle) };
}

// ---------------------------------------------------------------------------
// Compile-out behavior (spec section 7.2): built-in sources require the `net`
// feature. Default `libskopli` returns Catalog + the NoBuiltinSources message.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "net"))]
#[test]
fn builtin_sources_without_net_is_catalog_error() {
    let opts = json!({ "builtinSources": true });
    let mut handle = std::ptr::null_mut();
    let c = cjson(&opts);
    let status = unsafe { ag_pricing_new(c.as_ptr(), c.as_bytes().len(), &mut handle) };
    assert_eq!(status, AgStatus::Catalog);
    assert!(handle.is_null());
    let msg = last_error().expect("error set");
    // Exact-equality against the stable contract literal (duplicated from
    // pricing_opts::NO_BUILTIN_SOURCES) so any wording/capitalization drift fails.
    const NO_BUILTIN_SOURCES: &str = "builtin pricing sources are unavailable: this build of skopli was compiled without the \"net\" feature";
    assert_eq!(msg, NO_BUILTIN_SOURCES, "message: {msg}");
}

#[cfg(feature = "net")]
#[test]
fn builtin_sources_with_net_compiles_and_builds_handle() {
    // Offline + no cache: no real network is hit, and the handle still builds.
    let opts = json!({ "builtinSources": true, "offline": true });
    let mut handle = std::ptr::null_mut();
    let c = cjson(&opts);
    let status = unsafe { ag_pricing_new(c.as_ptr(), c.as_bytes().len(), &mut handle) };
    assert_eq!(
        status,
        AgStatus::Ok,
        "pricing_new failed: {:?}",
        last_error()
    );
    assert!(!handle.is_null());
    unsafe { ag_pricing_free(handle) };
}

/// Hermetic proof that the net build's built-in-sources path FETCHES
/// end to end: a tiny local HTTP server serves the three canonical lifecycle
/// payloads. The built-in fetch is redirected at the server via the net-gated,
/// `doc(hidden)` `pricing_from_options_with_urls` helper (NOT the public options
/// grammar / C ABI - `sourceUrls` is not a public option and does not appear in
/// `skopli.h`). The parse must succeed, the resulting catalogs must list all
/// three sources, the server must receive exactly three requests (one per
/// uncached source), and a `pricing-<name>.json` cache file must be written for
/// each. No real network.
#[cfg(feature = "net")]
#[test]
fn builtin_sources_with_net_fetches_from_local_server() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use skopli::{SourceUrls, pricing_from_options_with_urls};

    let lifecycle = repo_root().join("golden").join("pricing").join("lifecycle");
    let load = |name: &str| -> String { std::fs::read_to_string(lifecycle.join(name)).unwrap() };
    // Path -> body; the server routes by request path to the matching payload.
    let openrouter = load("source-openrouter.json");
    let litellm = load("source-litellm.json");
    let modelsdev = load("source-modelsdev.json");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicU32::new(0));

    let server_hits = Arc::clone(&hits);
    let server = std::thread::spawn(move || {
        // Serve exactly three requests, then return.
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            let line = req.lines().next().unwrap_or("");
            let body = if line.contains("/openrouter") {
                &openrouter
            } else if line.contains("/litellm") {
                &litellm
            } else {
                &modelsdev
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().ok();
            server_hits.fetch_add(1, Ordering::SeqCst);
        }
    });

    let dir = std::env::temp_dir().join(format!("skopli-net-fetch-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();

    let base = format!("http://{addr}");
    let opts = json!({
        "builtinSources": true,
        "cacheDir": dir.to_string_lossy(),
    });
    let urls = SourceUrls {
        openrouter: format!("{base}/openrouter"),
        litellm: format!("{base}/litellm"),
        models_dev: format!("{base}/modelsdev"),
    };
    let pricing = pricing_from_options_with_urls(&opts, &urls).expect("pricing built");

    let sources: Vec<String> = pricing
        .catalog_infos()
        .iter()
        .map(|i| i.source.clone())
        .collect();

    server.join().unwrap();

    assert!(
        sources.iter().any(|s| s == "openrouter"),
        "sources: {sources:?}"
    );
    assert!(
        sources.iter().any(|s| s == "litellm"),
        "sources: {sources:?}"
    );
    assert!(
        sources.iter().any(|s| s == "models-dev"),
        "sources: {sources:?}"
    );

    // The fetcher was invoked exactly once per uncached source.
    assert_eq!(hits.load(Ordering::SeqCst), 3, "server received 3 requests");

    // Each source's cache file was written.
    for name in ["openrouter", "litellm", "models-dev"] {
        let path = dir.join(format!("pricing-{name}.json"));
        assert!(path.is_file(), "cache file written for {name}: {path:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Cache-hit coverage kept separate: a temp cacheDir pre-populated
/// with a fresh cache file per source means every source hits cache and no fetch
/// is attempted, yet `catalog_info` still lists all three.
#[cfg(feature = "net")]
#[test]
fn builtin_sources_with_net_loads_from_cache_without_network() {
    let lifecycle = repo_root().join("golden").join("pricing").join("lifecycle");
    let load = |name: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(lifecycle.join(name)).unwrap()).unwrap()
    };

    let dir = std::env::temp_dir().join(format!("skopli-net-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Cache stem per source is `pricing-<name>.json`; the built-in source names
    // are openrouter, litellm, models-dev.
    let write_cache = |source: &str, payload: Value| {
        let file = json!({ "fetchedAt": "2099-01-01T00:00:00.000Z", "payload": payload });
        std::fs::write(
            dir.join(format!("pricing-{source}.json")),
            serde_json::to_string(&file).unwrap(),
        )
        .unwrap();
    };
    write_cache("openrouter", load("source-openrouter.json"));
    write_cache("litellm", load("source-litellm.json"));
    write_cache("models-dev", load("source-modelsdev.json"));

    let opts = json!({
        "builtinSources": true,
        "cacheDir": dir.to_string_lossy(),
    });
    let mut handle = std::ptr::null_mut();
    let c = cjson(&opts);
    let status = unsafe { ag_pricing_new(c.as_ptr(), c.as_bytes().len(), &mut handle) };
    assert_eq!(
        status,
        AgStatus::Ok,
        "pricing_new failed: {:?}",
        last_error()
    );
    assert!(!handle.is_null());

    let infos = {
        let mut out = AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status = unsafe { ag_pricing_catalog_info(handle, &mut out) };
        assert_eq!(status, AgStatus::Ok);
        buf_to_value(out)
    };
    unsafe { ag_pricing_free(handle) };
    std::fs::remove_dir_all(&dir).ok();

    let sources: Vec<&str> = infos
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["source"].as_str().unwrap())
        .collect();
    assert!(sources.contains(&"openrouter"), "sources: {sources:?}");
    assert!(sources.contains(&"litellm"), "sources: {sources:?}");
    assert!(sources.contains(&"models-dev"), "sources: {sources:?}");
}

// ---------------------------------------------------------------------------
// Ownership: freeing the empty buffer and null string is a no-op
// ---------------------------------------------------------------------------

#[test]
fn freeing_empty_and_null_is_safe() {
    unsafe {
        ag_buf_free(AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        });
        ag_string_free(std::ptr::null_mut());
    }
}

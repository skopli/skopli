//! FFI-only helper for the JSON boundary: safely reading a nullable, caller-owned
//! UTF-8 JSON input buffer into a `serde_json::Value`. The shared
//! `context_from_options` / event / price / cache-dir parsing lives in
//! `skopli-wire`; only this pointer-and-length reader is capi-specific.

use std::ffi::c_char;
use std::slice;

use serde_json::Value;

use crate::abi::{AgStatus, set_last_error};

/// Read a nullable, caller-owned UTF-8 JSON buffer into a `serde_json::Value`.
/// `null`/empty input yields `Value::Null` (the "all defaults" contract). A
/// non-null pointer with invalid UTF-8 or malformed JSON is an
/// [`AgStatus::InvalidArgument`].
///
/// # Safety
/// When `ptr` is non-null it must point at `len` readable bytes.
pub(crate) unsafe fn read_json_opt(ptr: *const c_char, len: usize) -> Result<Value, AgStatus> {
    if ptr.is_null() || len == 0 {
        return Ok(Value::Null);
    }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            set_last_error(format!("options JSON is not valid UTF-8: {e}"));
            return Err(AgStatus::InvalidArgument);
        }
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v) => Ok(v),
        Err(e) => {
            set_last_error(format!("invalid JSON: {e}"));
            Err(AgStatus::InvalidArgument)
        }
    }
}

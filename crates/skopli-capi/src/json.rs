//! FFI-only helper for the JSON boundary: safely reading a nullable, caller-owned
//! UTF-8 JSON input buffer into a `serde_json::Value`. The shared
//! `context_from_options` / event / price / cache-dir parsing lives in
//! `skopli-wire`; only this pointer-and-length reader is capi-specific.

use std::ffi::c_char;
use std::slice;

use serde_json::Value;

use crate::abi::AgError;

/// Read a nullable, caller-owned UTF-8 JSON buffer into a `serde_json::Value`.
/// `null`/empty input yields `Value::Null` (the "all defaults" contract). A
/// non-null pointer with invalid UTF-8 or malformed JSON is an
/// `InvalidArgument` [`AgError`].
///
/// # Safety
/// When `ptr` is non-null it must point at `len` readable bytes.
pub(crate) unsafe fn read_json_opt(ptr: *const c_char, len: usize) -> Result<Value, AgError> {
    if ptr.is_null() || len == 0 {
        return Ok(Value::Null);
    }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
    let text = std::str::from_utf8(bytes)
        .map_err(|e| AgError::invalid(format!("options JSON is not valid UTF-8: {e}")))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str::<Value>(trimmed)
        .map_err(|e| AgError::invalid(format!("invalid JSON: {e}")))
}

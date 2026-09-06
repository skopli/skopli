//! Low-level ABI primitives: the library-owned buffer/string ownership model,
//! the status-code enum, the thread-local error channel, and the panic-safety
//! wrapper every `extern "C"` entry point runs its body inside.
//!
//! Ownership discipline (normative):
//! - Every `AgBuf` handed OUT of the library is heap-allocated by the library
//!   and MUST be freed with [`ag_buf_free`]; the caller never frees `ptr`
//!   directly.
//! - Every `char*` handed OUT via an out-parameter (`ag_last_error_message`,
//!   `ag_default_cache_dir`) is a library-owned NUL-terminated UTF-8 C string
//!   and MUST be freed with [`ag_string_free`].
//! - `ag_version` returns a `'static` pointer that is NEVER freed.
//! - Every input buffer/string is caller-owned and only borrowed for the
//!   duration of the call.
//!
//! Thread-safety: no process-global mutable state exists beyond the thread-local
//! error message, so every function is safe to call concurrently from different
//! threads. An `AgPricing` handle is `Send + Sync` (it holds only an immutable
//! set of catalogs) but callers must still not free it while another thread is
//! using it.

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Status returned by every fallible ABI function. Mirrors the
/// `AgStatus` contract (0 ok, 1 invalid-argument, 2 catalog, 3 internal).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgStatus {
    /// The call succeeded.
    Ok = 0,
    /// A caller-supplied argument was invalid (null where required, malformed
    /// UTF-8/JSON, an out-of-range date/tz, or a bad enum value).
    InvalidArgument = 1,
    /// A pricing/catalog operation failed (e.g. unparseable catalog JSON).
    Catalog = 2,
    /// An unexpected internal error, including a caught panic.
    Internal = 3,
}

/// A library-owned byte buffer handed out to the caller. `ptr` points at `len`
/// bytes of UTF-8 JSON (no trailing NUL); free it with [`ag_buf_free`]. A
/// zero/`len == 0` buffer has `ptr == null` and needs no free.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AgBuf {
    /// Pointer to the first byte, or null when `len == 0`.
    pub ptr: *mut u8,
    /// Length in bytes.
    pub len: usize,
}

impl AgBuf {
    /// The empty buffer (null pointer, zero length).
    pub(crate) fn empty() -> Self {
        AgBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
        }
    }

    /// Move a `Vec<u8>` into a library-owned buffer. The allocation is later
    /// reclaimed by [`ag_buf_free`], which reconstructs the exact `(ptr, len,
    /// cap)` triple - so the capacity is shrunk to the length first to keep the
    /// free unambiguous.
    pub(crate) fn from_vec(mut bytes: Vec<u8>) -> Self {
        bytes.shrink_to_fit();
        if bytes.is_empty() {
            return AgBuf::empty();
        }
        let len = bytes.len();
        // Post-`shrink_to_fit` the capacity may still exceed len for some
        // allocators, so pin it explicitly by leaking a boxed slice and
        // remembering len == cap on the free side.
        let boxed = bytes.into_boxed_slice();
        let ptr = Box::into_raw(boxed) as *mut u8;
        AgBuf { ptr, len }
    }

    /// Move a JSON string into a library-owned buffer.
    pub(crate) fn from_string(s: String) -> Self {
        AgBuf::from_vec(s.into_bytes())
    }
}

thread_local! {
    /// The last error message set on THIS thread, if any. Cleared to `None` at
    /// the start of every fallible entry point.
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Record `message` as this thread's last error detail (see
/// [`ag_last_error_message`]). Interior NULs are stripped so the C string is
/// always well-formed.
pub(crate) fn set_last_error(message: impl Into<String>) {
    let raw = message.into().replace('\0', " ");
    let c = CString::new(raw).unwrap_or_else(|_| CString::new("error").unwrap());
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(c));
}

/// Clear this thread's last error detail (called at the top of each fallible fn
/// so a stale message from a previous call cannot leak into a later success).
pub(crate) fn clear_last_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// Run `body` under `catch_unwind`, converting a panic into a set error message
/// plus [`AgStatus::Internal`]. This is the single place unwinding is stopped:
/// no panic ever crosses the FFI boundary. `body` returns the status it wants.
pub(crate) fn guard(body: impl FnOnce() -> AgStatus) -> AgStatus {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(status) => status,
        Err(payload) => {
            let detail = panic_detail(&payload);
            set_last_error(format!("panic: {detail}"));
            AgStatus::Internal
        }
    }
}

/// Like [`guard`] but for the meta functions that cannot fail and return a
/// scalar: on panic it returns `fallback` and records the detail.
pub(crate) fn guard_value<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(payload) => {
            let detail = panic_detail(&payload);
            set_last_error(format!("panic: {detail}"));
            fallback
        }
    }
}

fn panic_detail(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}

/// Free a library-owned buffer previously returned in an `AgBuf* out`. Passing a
/// zero buffer (`ptr == null`) is a safe no-op. Double-freeing or freeing a
/// buffer the library did not produce is undefined behavior.
///
/// # Safety
/// `buf` must be a buffer produced by this library and not yet freed, or the
/// empty buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_buf_free(buf: AgBuf) {
    let _ = guard(|| {
        if !buf.ptr.is_null() && buf.len != 0 {
            // Reconstruct the boxed slice of exactly `len` bytes and drop it.
            let slice = std::ptr::slice_from_raw_parts_mut(buf.ptr, buf.len);
            unsafe { drop(Box::from_raw(slice)) };
        }
        AgStatus::Ok
    });
}

/// Free a library-owned C string previously returned via a `char** out`. Passing
/// null is a safe no-op.
///
/// # Safety
/// `s` must be a string produced by this library (via `ag_last_error_message` or
/// `ag_default_cache_dir`) and not yet freed, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_string_free(s: *mut c_char) {
    let _ = guard(|| {
        if !s.is_null() {
            unsafe { drop(CString::from_raw(s)) };
        }
        AgStatus::Ok
    });
}

/// Write this thread's last error detail (a heap C string) to `*out`, or write
/// null when no error has been recorded on this thread. The written string is
/// library-owned; free it with [`ag_string_free`]. Returns
/// [`AgStatus::InvalidArgument`] if `out` is null.
///
/// # Safety
/// `out` must be a valid, writable `*mut *mut c_char` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ag_last_error_message(out: *mut *mut c_char) -> AgStatus {
    guard(|| {
        if out.is_null() {
            return AgStatus::InvalidArgument;
        }
        let dup: Option<CString> =
            LAST_ERROR.with(|slot| slot.borrow().as_ref().map(|c| c.to_owned()));
        let ptr = match dup {
            Some(c) => c.into_raw(),
            None => std::ptr::null_mut(),
        };
        unsafe { *out = ptr };
        AgStatus::Ok
    })
}

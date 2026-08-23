//! GVL (Global VM Lock) release helper.
//!
//! Sync-only v1, but every magnus entry point releases
//! the GVL (`rb_thread_call_without_gvl`) around the Rust work so other Ruby
//! threads make progress while the pure Rust core parses JSON and reads/rolls
//! up/prices. Only argument marshalling in and result marshalling out hold the
//! GVL; the closure passed to [`without_gvl`] runs with the GVL RELEASED, so it
//! must NOT touch the Ruby VM (it operates purely on owned Rust data).
//!
//! Implemented directly over `rb_sys::rb_thread_call_without_gvl` (magnus does
//! not expose a safe wrapper for it). The closure is boxed and passed as the
//! `void*` data; the trampoline reconstitutes it, runs it once, and returns the
//! (boxed) result through the same pointer channel.
//!
//! Panic safety: the core is a large body of parsing/pricing code and a bug
//! there would `panic!`. A panic unwinding through the `extern "C"` trampoline is
//! undefined behaviour and would abort the whole Ruby VM. The trampoline
//! therefore wraps `work()` in [`std::panic::catch_unwind`] (mirroring the capi
//! crate's `guard` boundary in `crates/skopli-capi/src/abi.rs`): a caught
//! panic is captured as a message and returned as [`Err`], so the GVL-holding
//! caller can surface it as a Ruby exception instead of aborting the VM.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// A panic caught inside the GVL-released work closure, carrying the extracted
/// message. The caller (which re-holds the GVL) turns this into a Ruby
/// exception.
pub(crate) struct GvlPanic {
    pub(crate) message: String,
}

/// Run `work` with the GVL released. `work` receives no Ruby access and must
/// operate only on data captured by-value; its return value is moved back to the
/// caller (which still holds the GVL) for marshalling into Ruby.
///
/// If `work` panics, the unwind is stopped at the FFI boundary (it never crosses
/// into `libruby`) and returned as [`Err`] carrying the panic message.
pub(crate) fn without_gvl<F, R>(work: F) -> Result<R, GvlPanic>
where
    F: FnOnce() -> R,
{
    // The closure and its slot for the return value travel through the void*.
    struct Payload<F, R> {
        work: Option<F>,
        result: Option<Result<R, GvlPanic>>,
    }

    unsafe extern "C" fn trampoline<F, R>(data: *mut c_void) -> *mut c_void
    where
        F: FnOnce() -> R,
    {
        // SAFETY: `data` is the `&mut Payload` we handed to
        // `rb_thread_call_without_gvl`; it outlives this call (it lives on our
        // stack frame below), and Ruby calls this exactly once.
        let payload = unsafe { &mut *(data as *mut Payload<F, R>) };
        let work = payload.work.take().expect("work closure run once");
        // Stop any panic here: unwinding across this `extern "C"` boundary into
        // libruby is UB and would abort the VM. Capture the message and hand it
        // back to the GVL-holding caller.
        let outcome = match catch_unwind(AssertUnwindSafe(work)) {
            Ok(value) => Ok(value),
            Err(cause) => Err(GvlPanic {
                message: panic_detail(&cause),
            }),
        };
        payload.result = Some(outcome);
        std::ptr::null_mut()
    }

    let mut payload: Payload<F, R> = Payload {
        work: Some(work),
        result: None,
    };

    // SAFETY: FFI call into libruby. `trampoline::<F, R>` matches the required
    // `unsafe extern "C" fn(*mut c_void) -> *mut c_void` signature; the data
    // pointer is a valid `&mut Payload` for the duration of the call. We pass a
    // null `unblock` function and null `unblock` data (no interrupt handling for
    // this bounded, non-blocking compute; the core work is CPU/disk-bound and
    // finite). rb-sys re-exports the CRuby symbol.
    unsafe {
        rb_sys::rb_thread_call_without_gvl(
            Some(trampoline::<F, R>),
            (&mut payload as *mut Payload<F, R>).cast::<c_void>(),
            None,
            std::ptr::null_mut(),
        );
    }

    payload.result.expect("work closure produced a result")
}

/// Extract a human-readable message from a caught panic payload (mirrors the
/// capi crate's `panic_detail`).
fn panic_detail(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}

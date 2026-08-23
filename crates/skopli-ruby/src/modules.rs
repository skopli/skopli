//! Error-mapping glue between the internal [`DomainError`](crate::DomainError)
//! and Ruby. A domain error becomes a single native `Skopli::NativeError`
//! (a `StandardError` subclass) whose message is prefixed with a machine
//! `kind` tag (`invalid_argument:` | `catalog:` | `internal:`); the pure-Ruby
//! facade strips the tag and re-raises the typed subclass (`InvalidArgumentError`
//! / `CatalogError` / `Error`). This keeps the Rust side to one native exception
//! type and no host-language idiom - the same shape the PyO3 binding uses.

use magnus::{Error, Ruby};

use crate::gvl::GvlPanic;
use crate::{DomainError, native_error_class};

impl From<GvlPanic> for DomainError {
    /// A panic escaping the GVL-released core work is surfaced as an internal
    /// domain error (which the facade re-raises as `Skopli::Error`), never as
    /// a VM abort.
    fn from(panic: GvlPanic) -> Self {
        DomainError {
            kind: ErrKind::Internal,
            message: format!("panic: {}", panic.message),
        }
    }
}

/// Flatten the `Result<Result<T, DomainError>, GvlPanic>` a GVL-released,
/// fallible core call produces: a caught panic becomes an internal
/// [`DomainError`].
pub(crate) fn flatten_fallible<T>(
    outcome: Result<Result<T, DomainError>, GvlPanic>,
) -> Result<T, DomainError> {
    match outcome {
        Ok(inner) => inner,
        Err(panic) => Err(panic.into()),
    }
}

/// Flatten the `Result<T, GvlPanic>` an infallible core call produces: a caught
/// panic becomes an internal [`DomainError`].
pub(crate) fn flatten_infallible<T>(outcome: Result<T, GvlPanic>) -> Result<T, DomainError> {
    outcome.map_err(Into::into)
}

/// The facade-facing error kind carried on the native exception.
#[derive(Clone, Copy)]
pub(crate) enum ErrKind {
    InvalidArgument,
    Catalog,
    Internal,
}

impl ErrKind {
    fn as_str(self) -> &'static str {
        match self {
            ErrKind::InvalidArgument => "invalid_argument",
            ErrKind::Catalog => "catalog",
            ErrKind::Internal => "internal",
        }
    }
}

/// Convert a `Result<T, DomainError>` into a magnus `Result<T, Error>`, raising
/// `Skopli::NativeError` with the `kind` tag prefixed onto the message so the
/// Ruby facade can dispatch into its typed hierarchy.
pub(crate) trait RubyResultExt<T> {
    fn into_ruby_err(self, ruby: &Ruby) -> Result<T, Error>;
}

impl<T> RubyResultExt<T> for Result<T, DomainError> {
    fn into_ruby_err(self, ruby: &Ruby) -> Result<T, Error> {
        self.map_err(|e| {
            // `<kind>:<message>` - the facade splits on the first colon.
            let tagged = format!("{}:{}", e.kind.as_str(), e.message);
            Error::new(native_error_class(ruby), tagged)
        })
    }
}

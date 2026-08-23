//! Error-mapping glue between the internal [`DomainError`](crate::DomainError)
//! and Python. A domain error becomes an `SkopliError` instance carrying a
//! `kind` attribute (`"invalid_argument"` | `"catalog"` | `"internal"`); the
//! pure-Python facade inspects `kind` and re-raises the typed subclass
//! (`InvalidArgumentError` / `CatalogError`). This keeps the Rust side to one
//! native exception type and no host-language idiom.

use pyo3::prelude::*;

use crate::{DomainError, SkopliError};

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

/// Convert a `Result<T, DomainError>` into a `PyResult<T>`, building an
/// `SkopliError` with a `kind` attribute set for the Python facade.
pub(crate) trait PyResultExt<T> {
    fn into_py_err(self, py: Python<'_>) -> PyResult<T>;
}

impl<T> PyResultExt<T> for Result<T, DomainError> {
    fn into_py_err(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| {
            let err = SkopliError::new_err(e.message);
            // Attach `kind` to the exception instance so the facade can dispatch.
            let _ = err.value(py).setattr("kind", e.kind.as_str());
            err
        })
    }
}

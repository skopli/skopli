"""The exception hierarchy.

``SkopliError`` is the base; ``InvalidArgumentError`` (also a ``ValueError``,
for programmer-misuse idiom) and ``CatalogError`` are the two concrete kinds. The
native module raises a single exception carrying a ``kind`` attribute; the facade
re-raises it as the matching subclass via :func:`raise_from_native`.
"""

from __future__ import annotations

from typing import NoReturn


class SkopliError(Exception):
    """Base for every error raised by skopli."""


class InvalidArgumentError(SkopliError, ValueError):
    """A bad argument: an invalid date/tz, malformed input, unknown dimension."""


class CatalogError(SkopliError):
    """A pricing-catalog problem: a bad catalog/override, unknown source format."""


def raise_from_native(err: BaseException) -> NoReturn:
    """Re-raise a native ``_skopli.SkopliError`` as the typed subclass.

    Dispatches on the ``kind`` attribute the native side attaches
    (``"invalid_argument"`` | ``"catalog"`` | ``"internal"``); an unknown or
    missing kind falls back to the base ``SkopliError``.
    """
    message = str(err)
    kind = getattr(err, "kind", None)
    if kind == "invalid_argument":
        raise InvalidArgumentError(message) from err
    if kind == "catalog":
        raise CatalogError(message) from err
    raise SkopliError(message) from err

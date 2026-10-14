"""SQLTy - Type-safe SQL queries for Python."""

import importlib.metadata
import sys
from typing import get_args, overload

if sys.version_info >= (3, 15):
    from typing import TypeForm
else:
    from typing_extensions import Sentinel as sentinel
    from typing_extensions import TypeForm

try:
    __version__ = importlib.metadata.version(__name__)
except importlib.metadata.PackageNotFoundError:
    __version__ = "0.0.0"  # Fallback for development mode

__all__ = ["SQL", "UNTYPED", "row_form"]


class SQL[T](str):
    pass


UNTYPED = sentinel("UNTYPED")
"""Row type of a query sqlty could not infer: ``SQL[UNTYPED]``.

Unlike ``Any``, type checkers reject using an ``UNTYPED`` row as a concrete
type. ``sqlty rewrite --untyped sentinel`` emits it; ``sqlty check`` flags it
(``untyped-query``) until casts or a schema make the row type known.
"""


_MISSING: object = object()


@overload
def row_form[T](query: SQL[T], /) -> TypeForm[T]: ...


@overload
def row_form[T, D](query: SQL[T], default: D, /) -> TypeForm[T] | D: ...


def row_form(query: str, default: object = _MISSING, /) -> object:
    """Return the row type ``T`` a ``SQL[T](...)`` instance was built with.

    The type is read from ``__orig_class__``, so it exists only for queries
    constructed as ``SQL[T](...)``. Plain strings, annotation-only
    ``q: SQL[T] = "..."`` and stripped sources carry no runtime type: these
    raise ``TypeError`` unless a ``default`` is given.

    Example:
        >>> row_form(SQL[tuple[int, str]]("--sql\\nSELECT 1, 'a'"))
        tuple[int, str]
        >>> row_form(SQL("--sql\\nSELECT 1"), None) is None
        True
    """
    orig = getattr(query, "__orig_class__", None)
    if orig is not None and (args := get_args(orig)):
        return args[0]
    if default is not _MISSING:
        return default
    raise TypeError(f"{type(query).__name__} has no runtime row type; construct it as SQL[T](...)")

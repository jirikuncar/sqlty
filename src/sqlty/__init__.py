"""SQLTy - Type-safe SQL queries for Python."""

import sys
from string.templatelib import Template
from typing import get_args, overload

if sys.version_info >= (3, 15):
    from typing import TypeForm
else:
    from typing_extensions import Sentinel as sentinel
    from typing_extensions import TypeForm


def __getattr__(name: str) -> str:
    # ``__version__`` is looked up lazily: importlib.metadata costs ~10 ms to import.
    if name == "__version__":
        import importlib.metadata

        try:
            version = importlib.metadata.version(__name__)
        except importlib.metadata.PackageNotFoundError:
            version = "0.0.0"  # Fallback for development mode
        globals()["__version__"] = version
        return version
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


__all__ = ["SQL", "UNTYPED", "SQLTemplate", "row_form"]


class SQL[T](str):
    pass


class SQLTemplate[T]:
    """A t-string query, ``SQLTemplate[T](t"--sql\\n...")``, whose rows are ``T``.

    ``string.templatelib.Template`` cannot be subclassed, so the template is kept in
    ``template``: pass that to a driver that runs templates (psycopg >= 3.3), or the
    whole query to the ``sqlty.drivers.psycopg`` helpers. ``row_form`` reads ``T``.

    Example:
        >>> user_id = 1
        >>> query = SQLTemplate[tuple[int]](t"--sql\\nSELECT id::INTEGER FROM users WHERE id = {user_id}")
        >>> query.template.values
        (1,)
        >>> row_form(query)
        tuple[int]
    """

    __slots__ = ("__orig_class__", "template")

    def __init__(self, template: Template, /) -> None:
        self.template = template

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.template!r})"


UNTYPED = sentinel("UNTYPED")
"""Row type of a query sqlty could not infer: ``SQL[UNTYPED]``.

Unlike ``Any``, type checkers reject using an ``UNTYPED`` row as a concrete
type. ``sqlty rewrite --untyped sentinel`` emits it; ``sqlty check`` flags it
(``untyped-query``) until casts or a schema make the row type known.
"""


_MISSING: object = object()


@overload
def row_form[T](query: SQL[T] | SQLTemplate[T], /) -> TypeForm[T]: ...


@overload
def row_form[T, D](query: SQL[T] | SQLTemplate[T], default: D, /) -> TypeForm[T] | D: ...


def row_form(query: object, default: object = _MISSING, /) -> object:
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

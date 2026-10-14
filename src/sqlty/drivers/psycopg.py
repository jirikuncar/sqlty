"""Typed psycopg3 helpers driven by the row type of ``SQL[T]``."""

import sys
from collections.abc import Mapping, Sequence
from typing import Any, LiteralString, cast, get_origin, is_typeddict, overload

if sys.version_info >= (3, 15):
    from typing import TypeForm
else:
    from typing_extensions import TypeForm

from psycopg import Connection, Cursor
from psycopg.abc import Params
from psycopg.rows import BaseRowFactory, class_row, dict_row, tuple_row

from sqlty import SQL, row_form

__all__ = ["execute", "row_factory", "scalar"]


def row_factory[T](form: TypeForm[T]) -> BaseRowFactory[T]:
    """Return the psycopg row factory producing rows of type ``form``.

    - ``tuple[...]`` -> ``tuple_row``
    - ``TypedDict``, ``dict[...]`` -> ``dict_row``
    - other classes (dataclass, NamedTuple, Pydantic model, ...) -> ``class_row``

    Example:
        >>> row_factory(tuple[int, str]) is tuple_row
        True
    """
    origin = get_origin(form) or form
    if origin is tuple:
        return cast(BaseRowFactory[T], tuple_row)
    if origin is dict or is_typeddict(form):
        return cast(BaseRowFactory[T], dict_row)
    if isinstance(form, type):
        return class_row(form)
    raise TypeError(f"no psycopg row factory for {form!r}")


def execute[T](conn: Connection[Any], query: SQL[T], params: Params | None = None) -> Cursor[T]:
    """Execute ``query`` on a cursor whose rows are typed as ``T``.

    Queries without a runtime row type (see ``sqlty.row_form``) yield tuples.

    Example:
        >>> rows: list[User] = execute(conn, SQL[User]("--sql\\n...")).fetchall()  # doctest: +SKIP
    """
    form = row_form(query, cast(TypeForm[T], tuple))
    return conn.cursor(row_factory=row_factory(form)).execute(cast(LiteralString, query), params)


@overload
def scalar[T](
    conn: Connection[Any], query: SQL[tuple[T]], /, params: Params | None = None
) -> T: ...


@overload
def scalar[T](
    conn: Connection[Any],
    query: LiteralString | SQL[Any],
    form: TypeForm[T],
    /,
    params: Params | None = None,
) -> T: ...


def scalar(conn: Connection[Any], query: Any, /, *args: Any, **kwargs: Any) -> Any:
    """Return the single column of the first row of ``query``.

    The value is typed as ``T`` from ``SQL[tuple[T]]``, or as an explicit
    ``form`` for plain strings and narrowing. Any type expression works,
    including unions and ``Literal``. A query yielding no rows raises
    ``LookupError``, so ``None`` always means SQL ``NULL``.

    Example:
        >>> email = scalar(conn, SQL[tuple[str | None]]("--sql\\nSELECT email::TEXT FROM users WHERE id = %s"), (1,))  # doctest: +SKIP
        >>> role = scalar(conn, "--sql\\nSELECT role::TEXT FROM users LIMIT 1", Literal["admin", "user"])  # doctest: +SKIP
    """
    params = kwargs.get("params")
    if args and isinstance(args[-1], Sequence | Mapping) and not isinstance(args[-1], str):
        params = args[-1]
    row = conn.cursor(row_factory=tuple_row).execute(query, params).fetchone()
    if row is None:
        raise LookupError("query returned no rows")
    (value,) = row
    return value

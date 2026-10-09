"""Typed psycopg3 helpers driven by the row type of ``SQL[T]``."""

from typing import Any, LiteralString, TypeForm, cast, get_origin, is_typeddict

from psycopg import Connection, Cursor
from psycopg.abc import Params
from psycopg.rows import BaseRowFactory, class_row, dict_row, tuple_row

from sqlty import SQL, row_form

__all__ = ["execute", "row_factory"]


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

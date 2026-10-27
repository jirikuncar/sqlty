"""Typed psycopg3 helpers driven by the row type of ``SQL[T]``."""

import sys
from collections.abc import Mapping, Sequence
from typing import (
    Any,
    Final,
    LiteralString,
    cast,
    get_origin,
    is_protocol,
    is_typeddict,
    overload,
)

if sys.version_info >= (3, 15):
    from typing import TypeForm
else:
    from typing_extensions import TypeForm

from psycopg import Connection, Cursor
from psycopg.abc import Params
from psycopg.rows import BaseRowFactory, class_row, dict_row, namedtuple_row, tuple_row

from sqlty import SQL, Bound, SQLTemplate, row_form

__all__ = ["execute", "row_factory", "scalar"]


def row_factory[T](form: TypeForm[T]) -> BaseRowFactory[T]:
    """Return the psycopg row factory producing rows of type ``form``.

    - ``tuple[...]`` -> ``tuple_row``
    - ``TypedDict``, ``dict[...]`` -> ``dict_row``
    - ``Protocol`` -> ``namedtuple_row``: rows with every column as an attribute
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
    if isinstance(form, type) and is_protocol(form):
        return cast(BaseRowFactory[T], namedtuple_row)
    if isinstance(form, type):
        return class_row(form)
    raise TypeError(f"no psycopg row factory for {form!r}")


# mypy reads ``MYPY`` as true, pyright and ty as false. mypy cannot infer ``P`` from a
# TypedDict literal (python/mypy#19201), so it keeps the signature that does not check ``P``.
MYPY: Final = False

if MYPY:

    def execute[T](
        conn: Connection[Any],
        query: SQL[T] | SQLTemplate[T] | Bound[T],
        params: Params | None = None,
    ) -> Cursor[T]:
        return _execute(conn, query, params)

else:

    @overload
    def execute[T](
        conn: Connection[Any],
        query: SQL[T, tuple[()]] | SQLTemplate[T] | Bound[T],
        params: None = None,
    ) -> Cursor[T]: ...

    @overload
    def execute[T, P](conn: Connection[Any], query: SQL[T, P], params: P) -> Cursor[T]: ...

    def execute(conn: Connection[Any], query: Any, params: Any = None) -> Cursor[Any]:
        """Execute ``query`` on a cursor whose rows are typed as ``T``.

        Queries without a runtime row type (see ``sqlty.row_form``) yield tuples. A
        ``SQLTemplate`` runs its t-string, whose values are its parameters (psycopg >= 3.3).
        ``params`` are checked against the ``P`` of ``SQL[T, P]`` by pyright and ty; a
        ``Bound`` from ``SQL.bind`` carries its own, checked by every type checker.

        Example:
            >>> rows: list[User] = execute(conn, SQL[User]("--sql\\n...")).fetchall()  # doctest: +SKIP
            >>> rows = execute(conn, SQLTemplate[User](t"--sql\\n... WHERE id = {user_id}")).fetchall()  # doctest: +SKIP
            >>> rows = execute(conn, by_id.bind({"id": 1})).fetchall()  # doctest: +SKIP
        """
        return _execute(conn, query, params)


# ``params`` is not checked against ``P`` here: with ``P = Any``, ty finds a ``P`` overload
# ambiguous with the ``form`` one and loses ``T``. ``query.bind(params)`` checks it.
@overload
def scalar[T](
    conn: Connection[Any],
    query: SQL[tuple[T]] | SQLTemplate[tuple[T]] | Bound[tuple[T]],
    /,
    params: Params | None = None,
) -> T: ...


@overload
def scalar[T](
    conn: Connection[Any],
    query: LiteralString | SQL[Any] | SQLTemplate[Any],
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
        >>> name = scalar(conn, name_by_id.bind((1,)))  # doctest: +SKIP
    """
    params = kwargs.get("params")
    if isinstance(query, Bound):
        query, params = query
    elif args and isinstance(args[-1], Sequence | Mapping) and not isinstance(args[-1], str):
        params = args[-1]
    if isinstance(query, SQLTemplate):
        query, params = query.template, None
    row = conn.cursor(row_factory=tuple_row).execute(query, params).fetchone()
    if row is None:
        raise LookupError("query returned no rows")
    (value,) = row
    return value


def _execute(conn: Connection[Any], query: Any, params: Any) -> Cursor[Any]:
    if isinstance(query, Bound):
        query, params = query
    form = row_form(query, tuple)
    cursor = conn.cursor(row_factory=row_factory(form))
    if isinstance(query, SQLTemplate):
        return cursor.execute(cast(Any, query.template))
    return cursor.execute(query, params)

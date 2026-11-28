"""Typed rustmy helpers driven by the row type of ``SQL[T]``, blocking and ``rustmy.aio``."""

import sys
from collections import namedtuple
from collections.abc import AsyncIterator, Callable, Iterator, Mapping, Sequence
from functools import lru_cache
from typing import (
    Any,
    Final,
    LiteralString,
    Self,
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

from rustmy import aio
from rustmy.connections import Connection
from rustmy.converters import Params
from rustmy.cursors import Cursor, DictCursor

from sqlty import SQL, Bound, row_form

__all__ = ["AsyncRows", "Rows", "execute", "execute_async", "scalar", "scalar_async"]

type _Build = Callable[[Any], Any]
type _Plan = Callable[[list[str]], _Build]


def _itself(row: Any) -> Any:
    return row


def _plan(form: object) -> tuple[bool, _Plan]:
    """Whether rows are read as dicts, and how, given the column names, one becomes ``form``."""
    origin = get_origin(form) or form
    if origin is tuple:
        return False, lambda names: _itself
    if origin is dict or is_typeddict(form):
        return True, lambda names: _itself
    if isinstance(form, type) and is_protocol(form):
        return False, lambda names: _named(tuple(names))._make
    if isinstance(form, type):
        cls = form
        return True, lambda names: lambda row: cls(**row)
    raise TypeError(f"no rustmy row type for {form!r}")


@lru_cache(maxsize=512)
def _named(names: tuple[str, ...]) -> Any:
    return cast(Any, namedtuple("Row", names, rename=True))


def _prepare(query: Any, params: Any) -> tuple[Any, Any, bool, _Plan]:
    if isinstance(query, Bound):
        query, params = query
    as_dict, plan = _plan(row_form(query, tuple))
    return query, params, as_dict, plan


def _builder(cursor: Any, plan: _Plan) -> _Build:
    return plan([column[0] for column in cursor.description or ()])


class Rows[T]:
    """The rows of a query run with ``execute``, as ``T``; ``cursor`` is rustmy's.

    A context manager, closing the cursor on exit.
    """

    def __init__(self, cursor: Cursor, plan: _Plan) -> None:
        self.cursor = cursor
        self._build = _builder(cursor, plan)

    def fetchone(self) -> T | None:
        row = self.cursor.fetchone()
        return None if row is None else self._build(row)

    def fetchmany(self, size: int | None = None) -> list[T]:
        return [self._build(row) for row in self.cursor.fetchmany(size)]

    def fetchall(self) -> list[T]:
        return [self._build(row) for row in self.cursor.fetchall()]

    def __iter__(self) -> Iterator[T]:
        return (self._build(row) for row in self.cursor)

    def close(self) -> None:
        self.cursor.close()

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()


class AsyncRows[T]:
    """The rows of a query run with ``execute_async``, as ``T``; ``cursor`` is rustmy's."""

    def __init__(self, cursor: aio.Cursor, plan: _Plan) -> None:
        self.cursor = cursor
        self._build = _builder(cursor, plan)

    async def fetchone(self) -> T | None:
        row = await self.cursor.fetchone()
        return None if row is None else self._build(row)

    async def fetchmany(self, size: int | None = None) -> list[T]:
        rows = await self.cursor.fetchmany(size)
        return [self._build(row) for row in rows]

    async def fetchall(self) -> list[T]:
        return [self._build(row) for row in await self.cursor.fetchall()]

    async def __aiter__(self) -> AsyncIterator[T]:
        async for row in self.cursor:
            yield self._build(row)

    async def close(self) -> None:
        await self.cursor.close()

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.close()


# mypy reads ``MYPY`` as true, pyright and ty as false. mypy cannot infer ``P`` from a
# TypedDict literal (python/mypy#19201), so it keeps the signature that does not check ``P``.
MYPY: Final = False

if MYPY:

    def execute[T](conn: Connection, query: SQL[T] | Bound[T], params: Params = None) -> Rows[T]:
        return _execute(conn, query, params)

    async def execute_async[T](
        conn: aio.Connection, query: SQL[T] | Bound[T], params: Params = None
    ) -> AsyncRows[T]:
        return await _execute_async(conn, query, params)

else:

    @overload
    def execute[T](
        conn: Connection, query: SQL[T, tuple[()]] | Bound[T], params: None = None
    ) -> Rows[T]: ...

    @overload
    def execute[T, P](conn: Connection, query: SQL[T, P], params: P) -> Rows[T]: ...

    def execute(conn: Connection, query: Any, params: Any = None) -> Rows[Any]:
        """Run ``query`` on a cursor of ``conn`` whose rows are typed as ``T``.

        Rows are built from the query's runtime row type (see ``sqlty.row_form``): tuples
        for ``tuple[...]`` and queries without one, dicts for ``TypedDict`` and ``dict``,
        named tuples for a ``Protocol``, and ``T(**row)`` for any other class.

        Example:
            >>> users: list[User] = execute(conn, SQL[User]("--sql\\n...")).fetchall()  # doctest: +SKIP
            >>> user = execute(conn, by_id.bind((1,))).fetchone()  # doctest: +SKIP
        """
        return _execute(conn, query, params)

    @overload
    async def execute_async[T](
        conn: aio.Connection, query: SQL[T, tuple[()]] | Bound[T], params: None = None
    ) -> AsyncRows[T]: ...

    @overload
    async def execute_async[T, P](
        conn: aio.Connection, query: SQL[T, P], params: P
    ) -> AsyncRows[T]: ...

    async def execute_async(conn: aio.Connection, query: Any, params: Any = None) -> AsyncRows[Any]:
        """``execute`` for a ``rustmy.aio`` connection.

        Example:
            >>> users = await (await execute_async(conn, SQL[User]("--sql\\n..."))).fetchall()  # doctest: +SKIP
        """
        return await _execute_async(conn, query, params)


# ``params`` is not checked against ``P`` here: with ``P = Any``, ty finds a ``P`` overload
# ambiguous with the ``form`` one and loses ``T``. ``query.bind(params)`` checks it.
@overload
def scalar[T](
    conn: Connection, query: SQL[tuple[T]] | Bound[tuple[T]], /, params: Params = None
) -> T: ...


@overload
def scalar[T](
    conn: Connection,
    query: LiteralString | SQL[Any],
    form: TypeForm[T],
    /,
    params: Params = None,
) -> T: ...


def scalar(conn: Connection, query: Any, /, *args: Any, **kwargs: Any) -> Any:
    """Return the single column of the first row of ``query``.

    The value is typed as ``T`` from ``SQL[tuple[T]]``, or as an explicit ``form`` for
    plain strings and narrowing. A query yielding no rows raises ``LookupError``, so
    ``None`` always means SQL ``NULL``.

    Example:
        >>> email = scalar(conn, SQL[tuple[str | None]]("--sql\\nSELECT email FROM users WHERE id = %s"), (1,))  # doctest: +SKIP
        >>> name = scalar(conn, name_by_id.bind((1,)))  # doctest: +SKIP
    """
    query, params = _scalar_args(query, args, kwargs)
    with conn.cursor(Cursor) as cursor:
        cursor.execute(query, params)
        return _first(cursor.fetchone())


@overload
async def scalar_async[T](
    conn: aio.Connection, query: SQL[tuple[T]] | Bound[tuple[T]], /, params: Params = None
) -> T: ...


@overload
async def scalar_async[T](
    conn: aio.Connection,
    query: LiteralString | SQL[Any],
    form: TypeForm[T],
    /,
    params: Params = None,
) -> T: ...


async def scalar_async(conn: aio.Connection, query: Any, /, *args: Any, **kwargs: Any) -> Any:
    """``scalar`` for a ``rustmy.aio`` connection.

    Example:
        >>> name = await scalar_async(conn, name_by_id.bind((1,)))  # doctest: +SKIP
    """
    query, params = _scalar_args(query, args, kwargs)
    async with conn.cursor(aio.Cursor) as cursor:
        await cursor.execute(query, params)
        return _first(await cursor.fetchone())


def _execute(conn: Connection, query: Any, params: Any) -> Rows[Any]:
    query, params, as_dict, plan = _prepare(query, params)
    cursor = conn.cursor(DictCursor if as_dict else Cursor)
    cursor.execute(query, params)
    return Rows(cursor, plan)


async def _execute_async(conn: aio.Connection, query: Any, params: Any) -> AsyncRows[Any]:
    query, params, as_dict, plan = _prepare(query, params)
    cursor = conn.cursor(aio.DictCursor if as_dict else aio.Cursor)
    await cursor.execute(query, params)
    return AsyncRows(cursor, plan)


def _scalar_args(query: Any, args: tuple[Any, ...], kwargs: dict[str, Any]) -> tuple[Any, Any]:
    params = kwargs.get("params")
    if isinstance(query, Bound):
        query, params = query
    elif len(args) == 2 or (
        args and isinstance(args[-1], Sequence | Mapping) and not isinstance(args[-1], str)
    ):
        params = args[-1]
    return query, params


def _first(row: Any) -> Any:
    if row is None:
        raise LookupError("query returned no rows")
    (value,) = row
    return value

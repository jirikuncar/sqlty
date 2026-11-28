"""Typed asyncpg helpers driven by the row type of ``SQL[T]``.

The arguments after the query are its ``$1``, ``$2``, ... parameters, as asyncpg takes them.
asyncpg takes only an exact ``str`` as a query, so the helpers pass ``str(query)``.
"""

import sys
from collections import namedtuple
from collections.abc import Callable, Mapping
from functools import cache
from typing import Any, Protocol, cast, get_origin, is_protocol, is_typeddict

if sys.version_info >= (3, 15):
    from typing import TypeForm
else:
    from typing_extensions import TypeForm

from sqlty import SQL, Bound, row_form

__all__ = ["Executor", "execute", "fetch", "fetchrow", "fetchval", "row_builder"]

type Record = Mapping[str, Any]


class Executor(Protocol):
    """What runs a query: an ``asyncpg.Connection``, ``Pool`` or pooled connection."""

    async def fetch(self, query: str, *args: Any, timeout: float | None = None) -> list[Any]: ...

    async def fetchrow(self, query: str, *args: Any, timeout: float | None = None) -> Any: ...

    async def execute(self, query: str, *args: Any, timeout: float | None = None) -> str: ...


def row_builder[T](form: TypeForm[T]) -> Callable[[Record], T]:
    """Return the function that builds an ``asyncpg.Record`` as a row of type ``form``.

    - ``tuple[...]``, ``NamedTuple`` -> the values in order
    - ``TypedDict``, ``dict[...]`` -> a ``dict`` of the columns
    - ``Protocol`` -> a named tuple with every column as an attribute
    - other classes (dataclass, Pydantic model, ...) -> ``form(**columns)``

    Example:
        >>> row_builder(tuple[int, str])({"id": 1, "name": "a"})
        (1, 'a')
    """
    origin = get_origin(form) or form
    if origin is tuple:
        return lambda record: cast(T, tuple(record.values()))
    if origin is dict or is_typeddict(form):
        return lambda record: cast(T, dict(record))
    if isinstance(form, type) and issubclass(form, tuple):
        return lambda record: form(*record.values())
    if isinstance(form, type) and is_protocol(form):
        return lambda record: cast(T, _namedtuple(tuple(record.keys()))(*record.values()))
    if isinstance(form, type):
        return lambda record: form(**record)
    raise TypeError(f"no asyncpg row builder for {form!r}")


@cache
def _namedtuple(names: tuple[str, ...]) -> Callable[..., tuple[Any, ...]]:
    return namedtuple("Row", names, rename=True)  # noqa: PYI024


def _unbind(
    query: SQL[Any] | Bound[Any], args: tuple[Any, ...]
) -> tuple[SQL[Any], tuple[Any, ...]]:
    if not isinstance(query, Bound):
        return query, args
    if not isinstance(query.params, tuple | list):
        raise TypeError(f"asyncpg takes positional parameters, not {type(query.params).__name__}")
    return query.query, (*query.params, *args)


async def fetch[T](
    conn: Executor, query: SQL[T] | Bound[T], *args: Any, timeout: float | None = None
) -> list[T]:
    """Run ``query`` and return its rows as ``T``.

    Queries without a runtime row type (see ``sqlty.row_form``) yield tuples.

    Example:
        >>> users = await fetch(conn, SQL[User]("--sql\\nSELECT id, name FROM users WHERE team = $1"), 7)  # doctest: +SKIP
        >>> users = await fetch(conn, by_team.bind((7,)))  # doctest: +SKIP
    """
    sql, values = _unbind(query, args)
    build = row_builder(row_form(sql, tuple))
    return [build(record) for record in await conn.fetch(str(sql), *values, timeout=timeout)]


async def fetchrow[T](
    conn: Executor, query: SQL[T] | Bound[T], *args: Any, timeout: float | None = None
) -> T | None:
    """Run ``query`` and return its first row as ``T``, or ``None`` when it returns none.

    Example:
        >>> user = await fetchrow(conn, SQL[User]("--sql\\nSELECT id, name FROM users WHERE id = $1"), 1)  # doctest: +SKIP
    """
    sql, values = _unbind(query, args)
    record = await conn.fetchrow(str(sql), *values, timeout=timeout)
    return None if record is None else row_builder(row_form(sql, tuple))(record)


async def fetchval[T](
    conn: Executor, query: SQL[tuple[T]] | Bound[tuple[T]], *args: Any, timeout: float | None = None
) -> T:
    """Return the single column of the first row of ``query``.

    A query yielding no rows raises ``LookupError``, so ``None`` always means SQL ``NULL``,
    and a row of more than one column raises ``ValueError``.

    Example:
        >>> name = await fetchval(conn, SQL[tuple[str]]("--sql\\nSELECT name FROM users WHERE id = $1"), 1)  # doctest: +SKIP
    """
    sql, values = _unbind(query, args)
    record = await conn.fetchrow(str(sql), *values, timeout=timeout)
    if record is None:
        raise LookupError("query returned no rows")
    (value,) = record.values()
    return cast(T, value)


async def execute(
    conn: Executor, query: SQL[Any] | Bound[Any], *args: Any, timeout: float | None = None
) -> str:
    """Run ``query`` and return its status, such as ``"DELETE 1"``.

    Example:
        >>> await execute(conn, SQL("--sql\\nDELETE FROM users WHERE id = $1"), 1)  # doctest: +SKIP
        'DELETE 1'
    """
    sql, values = _unbind(query, args)
    return await conn.execute(str(sql), *values, timeout=timeout)

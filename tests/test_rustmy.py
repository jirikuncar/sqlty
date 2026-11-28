"""``sqlty.drivers.rustmy`` and the rustmy calls ``sqlty check`` reports.

The helpers run on a stand-in connection serving fixed rows, and on ``SQLTY_MYSQL_DSN``.
Each call case is one call on a rustmy cursor. The source holding the call is checked with
``check_source``, then the same source runs against ``SQLTY_MYSQL_DSN`` with the same
schema, as a temporary table: a call sqlty reports must fail there, and a call it leaves
alone must not. rustmy comes from the ``rustmy`` dependency group.
"""

from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator, Iterator
from contextlib import closing
from dataclasses import dataclass
from typing import Any, NamedTuple, Protocol, TypedDict, cast

import pytest
import rustmy
from rustmy import aio
from rustmy.cursors import DictCursor

from sqlty import SQL, core
from sqlty.drivers.rustmy import execute, execute_async, scalar, scalar_async

SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);"

SOURCE = """\
import rustmy


def run(conn: rustmy.Connection) -> None:
    cur = conn.cursor(prepared={prepared})
    {call}
"""

# Each call, and the rules sqlty reports for it: none when it is left alone, and the call
# fails at run time when one starts with `param-`.
CASES: list[tuple[str, list[str]]] = [
    ('cur.execute("SELECT id FROM users WHERE id = %s", (1,))', []),
    ('cur.execute("SELECT id FROM users WHERE id = %s", 1)', []),
    ('cur.execute("SELECT id FROM users WHERE name = %s", "a")', []),
    (
        'cur.execute("SELECT id FROM users WHERE id = %s AND age = %s", (1,))',
        ["param-count-mismatch"],
    ),
    ('cur.execute("SELECT id FROM users WHERE id = %s AND age = %s", 1)', ["param-count-mismatch"]),
    ('cur.execute("SELECT id FROM users WHERE id = %s", (1, 2))', ["param-count-mismatch"]),
    ('cur.execute("SELECT id FROM users WHERE id = %s")', ["param-count-mismatch"]),
    ('cur.execute("SELECT id FROM users WHERE id = %s", None)', ["param-count-mismatch"]),
    ('cur.execute("SELECT id FROM users", ())', []),
    ('cur.execute("SELECT id FROM users", (1,))', ["param-count-mismatch"]),
    ('cur.execute("SELECT id FROM users WHERE id = %(id)s", {"id": 1})', []),
    (
        'cur.execute("SELECT id FROM users WHERE id = %(id)s", {"ident": 1})',
        ["extra-param", "param-count-mismatch"],
    ),
    ('cur.execute("SELECT id FROM users WHERE id = %(id)s", {"id": 1, "x": 2})', ["extra-param"]),
    ('cur.execute("SELECT id FROM users WHERE id = %(id)s", 1)', ["param-type-mismatch"]),
    ('cur.execute("SELECT id FROM users WHERE id = %s", args=(1, 2))', ["param-count-mismatch"]),
    ('cur.executemany("DELETE FROM users WHERE id = %s", [1, 2])', []),
    (
        'cur.executemany("DELETE FROM users WHERE id = %s", [(1,), (2, 3)])',
        ["param-count-mismatch"],
    ),
    (
        'cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2,)])',
        ["param-count-mismatch"],
    ),
    (
        'cur.executemany("DELETE FROM users WHERE id = %s", seq_of_args=[(1, 2)])',
        ["param-count-mismatch"],
    ),
]


@pytest.fixture(scope="module")
def conn(mysql_connect_args: dict[str, Any]) -> Iterator[rustmy.Connection]:
    with closing(rustmy.connect(**mysql_connect_args)) as conn:
        with conn.cursor() as cursor:
            cursor.execute(SCHEMA.replace("CREATE TABLE", "CREATE TEMPORARY TABLE"))
        yield conn


def _reported(source: str) -> list[str]:
    diagnostics = core.check_source(source, core.Settings(dialect="mysql", schema_sql=SCHEMA))

    return [d["rule"] for d in diagnostics]


def _run(source: str, conn: rustmy.Connection) -> None:
    namespace: dict[str, Any] = {}
    exec(compile(source, "<rustmy case>", "exec"), namespace)

    namespace["run"](conn)


@pytest.mark.parametrize("prepared", [False, True])
@pytest.mark.parametrize(("call", "rules"), CASES)
def test_reported_calls_fail_at_run_time(
    conn: rustmy.Connection, call: str, rules: list[str], prepared: bool
) -> None:
    source = SOURCE.format(call=call, prepared=prepared)

    reported = _reported(source)

    assert sorted(reported) == rules

    if not any(rule.startswith("param-") for rule in rules):
        _run(source, conn)
    else:
        with pytest.raises(rustmy.Error):
            _run(source, conn)


@dataclass
class UserDC:
    id: int
    name: str


class UserNT(NamedTuple):
    id: int
    name: str


class UserTD(TypedDict):
    id: int
    name: str


class Named(Protocol):
    id: int
    name: str


QUERY = "-- sql\nSELECT 1 AS id, 'ada' AS name"
FORMS: list[tuple[Any, Any]] = [
    (tuple[int, str], (1, "ada")),
    (UserDC, UserDC(1, "ada")),
    (UserNT, UserNT(1, "ada")),
    (UserTD, {"id": 1, "name": "ada"}),
    (dict[str, Any], {"id": 1, "name": "ada"}),
]


class _Cursor:
    """A rustmy cursor's reading half, over ``rows`` of ``QUERY``'s columns."""

    def __init__(self, as_dict: bool, rows: list[tuple[Any, ...]]) -> None:
        self.description = [("id",), ("name",)] if rows and len(rows[0]) == 2 else [("n",)]
        names = [column[0] for column in self.description]
        self.rows = [dict(zip(names, row, strict=True)) if as_dict else row for row in rows]
        self.executed: list[tuple[str, Any]] = []
        self.closed = False

    def execute(self, query: str, params: Any = None) -> int:
        self.executed.append((query, params))
        return len(self.rows)

    def fetchone(self) -> Any:
        return self.rows.pop(0) if self.rows else None

    def fetchmany(self, size: int | None = None) -> tuple[Any, ...]:
        taken, self.rows = self.rows[: size or 1], self.rows[size or 1 :]
        return tuple(taken)

    def fetchall(self) -> tuple[Any, ...]:
        taken, self.rows = self.rows, []
        return tuple(taken)

    def __iter__(self) -> Iterator[Any]:
        return iter(self.fetchall())

    def __enter__(self) -> _Cursor:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def close(self) -> None:
        self.closed = True


class _AsyncCursor:
    def __init__(self, cursor: _Cursor) -> None:
        self.sync = cursor
        self.description = cursor.description

    async def execute(self, query: str, params: Any = None) -> int:
        return self.sync.execute(query, params)

    async def fetchone(self) -> Any:
        return self.sync.fetchone()

    async def fetchmany(self, size: int | None = None) -> tuple[Any, ...]:
        return self.sync.fetchmany(size)

    async def fetchall(self) -> tuple[Any, ...]:
        return self.sync.fetchall()

    async def __aiter__(self) -> AsyncIterator[Any]:
        for row in self.sync.fetchall():
            yield row

    async def __aenter__(self) -> _AsyncCursor:
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.close()

    async def close(self) -> None:
        self.sync.close()


class _Connection:
    """Serves ``rows`` to every cursor, as dicts to a ``DictCursor``."""

    def __init__(self, rows: list[tuple[Any, ...]]) -> None:
        self.rows = rows
        self.cursors: list[_Cursor] = []

    def cursor(self, cursorclass: type[Any] | None = None) -> Any:
        as_dict = cursorclass in (DictCursor, aio.DictCursor)
        self.cursors.append(_Cursor(as_dict, list(self.rows)))
        return self.cursors[-1]


class _AsyncConnection(_Connection):
    def cursor(self, cursorclass: type[Any] | None = None) -> Any:
        return _AsyncCursor(super().cursor(cursorclass))


def _sync(rows: list[tuple[Any, ...]]) -> Any:
    return cast(rustmy.Connection, _Connection(rows))


def _async(rows: list[tuple[Any, ...]]) -> Any:
    return cast(aio.Connection, _AsyncConnection(rows))


@pytest.mark.parametrize(("form", "expected"), FORMS)
def test_execute_builds_the_row_type(form: Any, expected: Any) -> None:
    rows = execute(_sync([(1, "ada"), (2, "bob")]), SQL[form](QUERY))

    assert rows.fetchone() == expected
    assert len(rows.fetchall()) == 1


def test_execute_reads_a_protocol_by_attribute() -> None:
    row: Named | None = execute(_sync([(1, "ada")]), SQL[Named](QUERY)).fetchone()

    assert row is not None
    assert (row.id, row.name) == (1, "ada")


def test_execute_without_row_form_yields_tuples() -> None:
    conn = _sync([(1, "ada"), (2, "bob"), (3, "cy")])

    rows = execute(conn, QUERY, ("x",))

    assert rows.fetchmany(2) == [(1, "ada"), (2, "bob")]
    assert list(rows) == [(3, "cy")]
    assert rows.fetchone() is None
    assert rows.cursor.executed == [(QUERY, ("x",))]


def test_execute_bound() -> None:
    conn = _sync([(1, "ada")])
    query = SQL[UserDC, tuple[int]]("-- sql\nSELECT id, name FROM users WHERE id = %s")

    assert execute(conn, query.bind((1,))).fetchall() == [UserDC(1, "ada")]
    assert conn.cursors[0].executed == [(query, (1,))]


def test_rows_close_their_cursor() -> None:
    with execute(_sync([(1, "ada")]), SQL[Named](QUERY)) as rows:
        assert [row.name for row in rows] == ["ada"]
    assert rows.cursor.closed


def test_execute_rejects_a_row_type_it_cannot_build() -> None:
    with pytest.raises(TypeError, match="no rustmy row type"):
        execute(_sync([]), SQL[list[int]](QUERY))


def test_scalar() -> None:
    query = SQL[tuple[int], tuple[int]]("-- sql\nSELECT %s + 1 AS n")

    assert scalar(_sync([(2,)]), query, (1,)) == 2
    assert scalar(_sync([(2,)]), query.bind((1,))) == 2
    assert scalar(_sync([(3,)]), "-- sql\nSELECT 3 AS n", int, params=None) == 3
    conn = _sync([(4,)])
    assert scalar(conn, "-- sql\nSELECT %s + 3 AS n", int, 1) == 4
    assert conn.cursors[0].executed == [("-- sql\nSELECT %s + 3 AS n", 1)]
    assert conn.cursors[0].closed
    with pytest.raises(LookupError, match="no rows"):
        scalar(_sync([]), query, (1,))


def test_async_helpers() -> None:
    async def run() -> None:
        rows = await execute_async(_async([(1, "ada"), (2, "bob"), (3, "cy")]), SQL[UserDC](QUERY))
        assert await rows.fetchone() == UserDC(1, "ada")
        assert await rows.fetchmany() == [UserDC(2, "bob")]
        assert [row async for row in rows] == [UserDC(3, "cy")]
        assert await rows.fetchall() == []
        async with await execute_async(_async([]), QUERY) as empty:
            assert await empty.fetchone() is None
        assert empty.cursor.sync.closed
        assert await scalar_async(_async([(4,)]), "-- sql\nSELECT 4 AS n", int) == 4
        with pytest.raises(LookupError):
            await scalar_async(_async([]), "-- sql\nSELECT 4 AS n", int)

    asyncio.run(run())


@pytest.mark.parametrize(("form", "expected"), FORMS)
def test_execute_on_mysql(conn: rustmy.Connection, form: Any, expected: Any) -> None:
    assert execute(conn, SQL[form](QUERY)).fetchall() == [expected]
    assert scalar(conn, "-- sql\nSELECT %s + 1", int, (1,)) == 2
    assert scalar(conn, "-- sql\nSELECT %s + 1", int, 1) == 2


def test_scalar_on_a_dict_cursor_connection(mysql_connect_args: dict[str, Any]) -> None:
    with closing(rustmy.connect(**mysql_connect_args, cursorclass=DictCursor)) as conn:
        assert scalar(conn, SQL[tuple[int]]("-- sql\nSELECT 5 AS n")) == 5


def test_async_helpers_on_mysql(mysql_connect_args: dict[str, Any]) -> None:
    async def run() -> None:
        async with aio.connect(**mysql_connect_args) as conn:
            rows = await execute_async(conn, SQL[Named](QUERY))
            assert [(row.id, row.name) async for row in rows] == [(1, "ada")]
            assert await scalar_async(conn, SQL[tuple[int]]("-- sql\nSELECT %s + 1"), (1,)) == 2
        async with aio.connect(**mysql_connect_args, cursorclass=aio.DictCursor) as conn:
            assert await scalar_async(conn, "-- sql\nSELECT 5 AS n", int) == 5

    asyncio.run(run())

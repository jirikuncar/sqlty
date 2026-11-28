"""The ``sqlty.drivers.asyncpg`` helpers, on an executor that returns mappings as asyncpg returns records."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any, NamedTuple, Protocol, TypedDict

import pytest

from sqlty import SQL
from sqlty.drivers.asyncpg import execute, fetch, fetchrow, fetchval, row_builder


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


QUERY = "--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id > $1"
ROWS = [{"id": 1, "name": "ada"}, {"id": 2, "name": "bob"}]


class Executor:
    """Like asyncpg, takes only an exact ``str`` as a query."""

    def __init__(self, rows: list[dict[str, Any]]) -> None:
        self.rows = rows
        self.calls: list[tuple[str, tuple[Any, ...], float | None]] = []

    async def fetch(self, query: str, *args: Any, timeout: float | None = None) -> list[Any]:
        assert type(query) is str
        self.calls.append((query, args, timeout))
        return self.rows

    async def fetchrow(self, query: str, *args: Any, timeout: float | None = None) -> Any:
        assert type(query) is str
        self.calls.append((query, args, timeout))
        return self.rows[0] if self.rows else None

    async def execute(self, query: str, *args: Any, timeout: float | None = None) -> str:
        assert type(query) is str
        self.calls.append((query, args, timeout))
        return f"DELETE {len(self.rows)}"


# fmt: off
@pytest.mark.parametrize(("form", "row"), [
    (tuple[int, str], (1, "ada")),
    (UserNT, UserNT(1, "ada")),
    (UserTD, {"id": 1, "name": "ada"}),
    (dict[str, Any], {"id": 1, "name": "ada"}),
    (UserDC, UserDC(1, "ada")),
])
# fmt: on
def test_row_builder(form: Any, row: Any) -> None:
    built = row_builder(form)(ROWS[0])
    assert built == row
    assert type(built) is type(row)


def test_row_builder_protocol() -> None:
    row: Any = row_builder(Named)({"id": 1, "name": "ada", "class": "x"})
    assert (row.id, row.name, row) == (1, "ada", (1, "ada", "x"))


def test_row_builder_rejects_a_non_type() -> None:
    with pytest.raises(TypeError, match="no asyncpg row builder"):
        row_builder(int | str)


def test_fetch_builds_rows_and_passes_args() -> None:
    conn = Executor(ROWS)
    rows = asyncio.run(fetch(conn, SQL[UserDC](QUERY), 0, timeout=2.0))
    assert rows == [UserDC(1, "ada"), UserDC(2, "bob")]
    assert conn.calls == [(QUERY, (0,), 2.0)]


def test_fetch_without_a_runtime_row_type_yields_tuples() -> None:
    assert asyncio.run(fetch(Executor(ROWS), SQL(QUERY), 0)) == [(1, "ada"), (2, "bob")]


def test_bound_params_come_before_args() -> None:
    conn = Executor(ROWS)
    query = SQL[UserTD]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id > $1 AND name <> $2")
    assert asyncio.run(fetch(conn, query.bind((0,)), "x")) == ROWS
    assert conn.calls == [(query, (0, "x"), None)]


@pytest.mark.parametrize("params", [{"id": 0}, None])
def test_bound_params_must_be_positional(params: Any) -> None:
    query = SQL[UserTD](QUERY)
    with pytest.raises(TypeError, match="positional parameters"):
        asyncio.run(fetch(Executor(ROWS), query.bind(params)))


def test_fetchval_of_two_columns() -> None:
    with pytest.raises(ValueError, match="too many values"):
        asyncio.run(fetchval(Executor(ROWS), SQL[tuple[int]](QUERY), 0))


def test_fetchrow() -> None:
    assert asyncio.run(fetchrow(Executor(ROWS), SQL[UserNT](QUERY), 0)) == UserNT(1, "ada")
    assert asyncio.run(fetchrow(Executor([]), SQL[UserNT](QUERY), 0)) is None


def test_fetchval() -> None:
    query = SQL[tuple[int | None]]("--sql\nSELECT age::INTEGER FROM users WHERE id = $1")
    assert asyncio.run(fetchval(Executor([{"age": None}]), query, 1)) is None
    with pytest.raises(LookupError, match="no rows"):
        asyncio.run(fetchval(Executor([]), query, 1))


def test_execute() -> None:
    conn = Executor(ROWS)
    assert asyncio.run(execute(conn, SQL("--sql\nDELETE FROM users WHERE id > $1"), 0)) == "DELETE 2"

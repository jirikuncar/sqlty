"""Row types flow from SQL[T] through the rustmy helpers.

mypy: ok
pyright: ok
ty: ok
"""

from dataclasses import dataclass
from typing import TypedDict, assert_type

import rustmy
from rustmy import aio

from sqlty import SQL
from sqlty.drivers.rustmy import AsyncRows, Rows, execute, execute_async, scalar, scalar_async


@dataclass
class User:
    id: int
    name: str


class ById(TypedDict):
    id: int


query = SQL[User]("-- sql\nSELECT id, name FROM users")
name_by_id = SQL[tuple[str], ById]("-- sql\nSELECT name FROM users WHERE id = %(id)s")


def fetch(conn: rustmy.Connection) -> None:
    assert_type(execute(conn, query), Rows[User])
    assert_type(execute(conn, query).fetchall(), list[User])
    assert_type(execute(conn, query).fetchone(), User | None)
    assert_type(execute(conn, name_by_id, {"id": 1}).fetchmany(2), list[tuple[str]])
    assert_type(execute(conn, name_by_id.bind({"id": 1})).fetchone(), tuple[str] | None)
    for user in execute(conn, query):
        assert_type(user, User)
    assert_type(scalar(conn, name_by_id.bind({"id": 1})), str)
    assert_type(scalar(conn, "-- sql\nSELECT COUNT(*) FROM users", int), int)


async def fetch_async(conn: aio.Connection) -> None:
    rows = await execute_async(conn, query)
    assert_type(rows, AsyncRows[User])
    assert_type(await rows.fetchall(), list[User])
    assert_type(await rows.fetchone(), User | None)
    async for user in rows:
        assert_type(user, User)
    assert_type(await scalar_async(conn, name_by_id.bind({"id": 1})), str)

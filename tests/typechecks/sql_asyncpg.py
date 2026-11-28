"""The asyncpg helpers type rows from SQL[T] and take an asyncpg connection or pool.

mypy: ok
pyright: ok
ty: ok
"""

from typing import NamedTuple, assert_type

import asyncpg

from sqlty import SQL
from sqlty.drivers.asyncpg import execute, fetch, fetchrow, fetchval


class User(NamedTuple):
    id: int
    name: str


async def main(conn: asyncpg.Connection, pool: asyncpg.Pool) -> None:
    by_team = SQL[User]("--sql\nSELECT id, name FROM users WHERE team = $1")
    assert_type(await fetch(conn, by_team, 7), list[User])
    assert_type(await fetch(pool, by_team.bind((7,))), list[User])
    assert_type(await fetchrow(conn, by_team, 7, timeout=1.0), User | None)
    assert_type(await fetchval(conn, SQL[tuple[str | None]]("--sql\nSELECT name FROM users")), str | None)
    assert_type(await execute(pool, SQL("--sql\nDELETE FROM users")), str)

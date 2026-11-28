import datetime
from dataclasses import dataclass
from typing import TypedDict

import rustmy
from rustmy import aio

from sqlty import SQL
from sqlty.drivers.rustmy import execute, execute_async, scalar


@dataclass
class User:
    id: int
    name: str
    email: str | None


class ById(TypedDict):
    id: int


by_id = SQL[User, ById]("""-- sql
SELECT id, name, email FROM users WHERE id = %(id)s""")

created = SQL[tuple[datetime.datetime], ById]("""-- sql
SELECT created_at FROM users WHERE id = %(id)s""")

everyone = SQL[User]("""-- sql
SELECT id, name, email FROM users ORDER BY id""")


def find(conn: rustmy.Connection, user_id: int) -> User | None:
    return execute(conn, by_id, {"id": user_id}).fetchone()


def signed_up(conn: rustmy.Connection, user_id: int) -> datetime.datetime:
    return scalar(conn, created.bind({"id": user_id}))


def rename(conn: rustmy.Connection, user_id: int, name: str) -> None:
    with conn.cursor() as cur:
        cur.execute("UPDATE users SET name = %s WHERE id = %s", (name, user_id))
    conn.commit()


async def users(pool: aio.Pool) -> list[User]:
    async with pool.acquire() as conn:
        rows = await execute_async(conn, everyone)
        return await rows.fetchall()

"""The psycopg helpers take the P of SQL[R, P], or a Bound from bind().

mypy: ok
pyright: ok
ty: ok
"""

from typing import Any, TypedDict, assert_type

from psycopg import Connection, Cursor

from sqlty import SQL
from sqlty.drivers.psycopg import execute, scalar


class ById(TypedDict):
    id: int


by_id = SQL[tuple[str], ById]("--sql\nSELECT name::TEXT FROM users WHERE id = %(id)s")
by_email = SQL[tuple[int], tuple[str | None]]("--sql\nSELECT id::INTEGER FROM users WHERE email = %s")
count = SQL[tuple[int], tuple[()]]("--sql\nSELECT count(*)::INTEGER FROM users")
old = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users WHERE id = %s")


def fetch(conn: Connection[Any]) -> None:
    assert_type(execute(conn, by_id.bind({"id": 1})), Cursor[tuple[str]])
    assert_type(execute(conn, by_id, {"id": 1}), Cursor[tuple[str]])
    assert_type(execute(conn, by_email, ("a@example.com",)), Cursor[tuple[int]])
    assert_type(execute(conn, by_email, params=(None,)), Cursor[tuple[int]])
    assert_type(execute(conn, count), Cursor[tuple[int]])
    assert_type(execute(conn, count, None), Cursor[tuple[int]])
    assert_type(execute(conn, old), Cursor[tuple[int]])
    assert_type(execute(conn, old, [1]), Cursor[tuple[int]])
    assert_type(execute(conn, old, params={"id": 1}), Cursor[tuple[int]])
    assert_type(scalar(conn, by_id.bind({"id": 1})), str)
    assert_type(scalar(conn, count), int)
    assert_type(scalar(conn, old, (1,)), int)

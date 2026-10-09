"""scalar() types its value from SQL[tuple[T]] or an explicit TypeForm.

mypy: ok
pyright: ok
ty: ok
"""

from typing import Any, Literal, assert_type

from psycopg import Connection

from sqlty import SQL
from sqlty.drivers.psycopg import scalar


def fetch(conn: Connection[Any]) -> None:
    email = SQL[tuple[str | None]]("--sql\nSELECT email::TEXT FROM users WHERE id = %s")
    assert_type(scalar(conn, email, (1,)), str | None)
    assert_type(scalar(conn, email, params=[1]), str | None)
    assert_type(scalar(conn, SQL[tuple[int]]("--sql\nSELECT count(*)::INTEGER FROM users")), int)

    role = "--sql\nSELECT role::TEXT FROM users LIMIT 1"
    assert_type(scalar(conn, role, Literal["admin", "user"]), Literal["admin", "user"])
    assert_type(scalar(conn, role, int | None), int | None)
    assert_type(scalar(conn, email, str, (1,)), str)
    assert_type(scalar(conn, email, str, params={"id": 1}), str)

"""Wrong parameters are errors; the comment of each line names the checkers that report it."""

from typing import Any, TypedDict

from psycopg import Connection

from sqlty import SQL
from sqlty.drivers.psycopg import execute


class ById(TypedDict):
    id: int


by_id = SQL[tuple[str], ById]("--sql\nSELECT name::TEXT FROM users WHERE id = %(id)s")
by_email = SQL[tuple[int], tuple[str | None]]("--sql\nSELECT id::INTEGER FROM users WHERE email = %s")
count = SQL[tuple[int], tuple[()]]("--sql\nSELECT count(*)::INTEGER FROM users")

by_id.bind({"id": "1"})  # error: mypy pyright ty
by_id.bind({"ident": 1})  # error: mypy pyright ty
by_id.bind((1,))  # error: mypy pyright ty
by_email.bind((b"a@example.com",))  # error: mypy pyright ty
by_email.bind(("a@example.com", 1))  # error: mypy pyright ty
count.bind((1,))  # error: mypy pyright ty


def nullable(query: SQL[Any, tuple[str | None]]) -> None: ...


nullable(SQL[Any, tuple[str]](""))  # error: mypy pyright ty


def fetch(conn: Connection[Any]) -> None:
    execute(conn, by_id, {"id": "1"})  # error: pyright ty
    execute(conn, by_id, {"ident": 1})  # error: pyright ty
    execute(conn, by_email, (b"a@example.com",))  # error: pyright ty
    execute(conn, by_email, ("a@example.com", 1))  # error: pyright ty
    execute(conn, by_email)  # error: pyright ty
    execute(conn, count, (1,))  # error: pyright ty

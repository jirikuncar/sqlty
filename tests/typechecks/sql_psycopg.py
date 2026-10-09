"""Row types flow from SQL[T] through the psycopg helpers.

mypy: ok
pyright: ok
ty: ok
"""

from dataclasses import dataclass
from typing import Any, NamedTuple, TypedDict, TypeForm, assert_type

from psycopg import Connection, Cursor
from psycopg.rows import BaseRowFactory

from sqlty import SQL, row_form
from sqlty.drivers.psycopg import execute, row_factory


@dataclass
class User:
    id: int
    name: str


class UserTD(TypedDict):
    id: int
    name: str


class UserNT(NamedTuple):
    id: int
    name: str


query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")

assert_type(row_form(query), TypeForm[User])
assert_type(row_form(query, None), TypeForm[User] | None)
assert_type(row_factory(User), BaseRowFactory[User])
assert_type(row_factory(UserNT), BaseRowFactory[UserNT])
assert_type(row_factory(UserTD), BaseRowFactory[UserTD])
assert_type(row_factory(tuple[int, str]), BaseRowFactory[tuple[int, str]])


def fetch(conn: Connection[Any]) -> None:
    assert_type(execute(conn, query), Cursor[User])
    assert_type(execute(conn, query).fetchall(), list[User])
    assert_type(execute(conn, query).fetchone(), User | None)
    pairs = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
    assert_type(execute(conn, pairs, {"id": 1}).fetchall(), list[tuple[int, str]])

"""Row types flow from SQLTemplate[T] through row_form and the psycopg helpers.

mypy: ok
pyright: ok
ty: ok
"""

import sys
from string.templatelib import Template
from typing import Any, TypedDict, assert_type

if sys.version_info >= (3, 15):
    from typing import TypeForm
else:
    from typing_extensions import TypeForm

from psycopg import Connection, Cursor

from sqlty import SQLTemplate, row_form
from sqlty.drivers.psycopg import execute, scalar


class User(TypedDict):
    id: int
    name: str


user_id = 1
query = SQLTemplate[User](t"--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = {user_id}")

assert_type(query, SQLTemplate[User])
assert_type(query.template, Template)
assert_type(row_form(query), TypeForm[User])


def fetch(conn: Connection[Any]) -> None:
    assert_type(execute(conn, query), Cursor[User])
    assert_type(execute(conn, query).fetchone(), User | None)
    count = SQLTemplate[tuple[int]](t"--sql\nSELECT count(*)::INTEGER FROM users WHERE id > {user_id}")
    assert_type(scalar(conn, count), int)

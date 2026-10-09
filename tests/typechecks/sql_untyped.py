"""SQL[UNTYPED] is a valid type expression, distinct from Any.

mypy: revealed: UNTYPED
pyright: revealed: UNTYPED
ty: revealed: UNTYPED
"""

from typing import assert_type

from sqlty import SQL, UNTYPED


def rows[T](query: SQL[T]) -> list[T]:
    return []


query = SQL[UNTYPED]("--sql\nDELETE FROM users")
assert_type(query, SQL[UNTYPED])
row = rows(query)[0]
assert_type(row, UNTYPED)
reveal_type(row)  # UNTYPED

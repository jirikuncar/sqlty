"""SQL[R, P] types the parameters of a query: bind() takes P, which is contravariant.

mypy: ok
pyright: ok
ty: ok
"""

from typing import Any, TypedDict, assert_type

from sqlty import SQL, Bound


class ById(TypedDict):
    id: int


by_id = SQL[tuple[str], ById]("--sql\nSELECT name::TEXT FROM users WHERE id = %(id)s")
by_email = SQL[tuple[int], tuple[str | None]]("--sql\nSELECT id::INTEGER FROM users WHERE email = %s")
count = SQL[tuple[int], tuple[()]]("--sql\nSELECT count(*)::INTEGER FROM users")
old = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users WHERE id = %s")

assert_type(by_id.bind({"id": 1}), Bound[tuple[str]])
assert_type(by_email.bind(("a@example.com",)), Bound[tuple[int]])
assert_type(by_email.bind((None,)), Bound[tuple[int]])
assert_type(count.bind(()), Bound[tuple[int]])
assert_type(old.bind(["anything", 1]), Bound[tuple[int]])
assert_type(old.bind({"any": "thing"}), Bound[tuple[int]])


def strict(query: SQL[Any, tuple[str]]) -> None: ...


def rows(query: SQL[tuple[int]]) -> None: ...


strict(by_email)
rows(by_email)
rows(count)

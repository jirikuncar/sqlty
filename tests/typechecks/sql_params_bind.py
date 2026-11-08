"""SQL[R, P] types the parameters of a query: bind() takes P and returns Bound[R, P], both
contravariant in P.

mypy: ok
pyright: ok
ty: ok
"""

from collections.abc import Mapping
from typing import Any, TypedDict, assert_type

from sqlty import SQL, Bound, NoParams


class ById(TypedDict):
    id: int


by_id = SQL[tuple[str], ById]("--sql\nSELECT name::TEXT FROM users WHERE id = %(id)s")
by_email = SQL[tuple[int], tuple[str | None]]("--sql\nSELECT id::INTEGER FROM users WHERE email = %s")
count = SQL[tuple[int], NoParams]("--sql\nSELECT count(*)::INTEGER FROM users")
old = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users WHERE id = %s")

assert_type(by_id.bind({"id": 1}), Bound[tuple[str], ById])
assert_type(by_email.bind(("a@example.com",)), Bound[tuple[int], tuple[str | None]])
assert_type(by_email.bind((None,)), Bound[tuple[int], tuple[str | None]])
assert_type(count.bind(()), Bound[tuple[int], NoParams])
assert_type(count.bind({}), Bound[tuple[int], NoParams])
assert_type(old.bind(["anything", 1]), Bound[tuple[int], Any])
assert_type(old.bind({"any": "thing"}), Bound[tuple[int], Any])


def strict(query: SQL[Any, tuple[str]]) -> None: ...


def rows(query: SQL[tuple[int]]) -> None: ...


strict(by_email)
rows(by_email)
rows(count)


def positional(query: SQL[Any, tuple[()]]) -> None: ...


positional(count)


def by_params(bound: Bound[Any, ById]) -> None: ...


def any_rows(bound: Bound[tuple[str]]) -> None: ...


bound = by_id.bind({"id": 1})
assert_type(bound.query, SQL[tuple[str]])
by_params(bound)
by_params(old.bind({"id": 1}))
by_params(SQL[tuple[int], Mapping[str, object]]("--sql\nSELECT 1").bind({"id": 1}))
any_rows(bound)

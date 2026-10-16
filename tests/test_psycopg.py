"""Tests for sqlty.row_form and the psycopg row factory helpers.

Database tests run when ``SQLTY_PG_DSN`` points to a PostgreSQL server.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Any, Literal, NamedTuple, Protocol, TypedDict

import pytest

psycopg = pytest.importorskip("psycopg")

from psycopg.rows import dict_row, namedtuple_row, tuple_row  # noqa: E402

from sqlty import SQL, SQLTemplate, row_form  # noqa: E402
from sqlty.drivers.psycopg import execute, row_factory, scalar  # noqa: E402


@dataclass
class UserDC:
    id: int
    name: str


class UserNT(NamedTuple):
    id: int
    name: str


class UserTD(TypedDict):
    id: int
    name: str


class Named(Protocol):
    id: int
    name: str


QUERY = "--sql\nSELECT 1::INTEGER AS id, 'ada'::TEXT AS name"


@pytest.mark.parametrize("form", [tuple[int, str], UserDC, UserNT, UserTD, list[int]])
def test_row_form(form: Any) -> None:
    assert row_form(SQL[form](QUERY)) == form


@pytest.mark.parametrize("query", [SQL(QUERY), SQL[int](QUERY) + " LIMIT 1", QUERY])
def test_row_form_missing(query: Any) -> None:
    with pytest.raises(TypeError, match="no runtime row type"):
        row_form(query)
    assert row_form(query, None) is None


@pytest.mark.parametrize(
    ("form", "factory"),
    [
        (tuple, tuple_row),
        (tuple[int, str], tuple_row),
        (tuple[int, ...], tuple_row),
        (dict[str, Any], dict_row),
        (UserTD, dict_row),
    ],
)
def test_row_factory_builtin(form: Any, factory: Any) -> None:
    assert row_factory(form) is factory


@pytest.mark.parametrize("form", [UserDC, UserNT])
def test_row_factory_class(form: Any) -> None:
    assert row_factory(form).__qualname__.startswith("class_row")


def test_row_factory_protocol() -> None:
    assert row_factory(Named) is namedtuple_row


@pytest.mark.parametrize("form", [int | str, list[int]])
def test_row_factory_unsupported(form: Any) -> None:
    with pytest.raises(TypeError, match="no psycopg row factory"):
        row_factory(form)


@pytest.fixture(scope="module")
def conn() -> Any:
    dsn = os.environ.get("SQLTY_PG_DSN")
    if not dsn:
        pytest.skip("SQLTY_PG_DSN not set")
    with psycopg.connect(dsn) as conn:
        yield conn


@pytest.mark.parametrize(
    ("form", "expected"),
    [
        (tuple[int, str], (1, "ada")),
        (UserDC, UserDC(1, "ada")),
        (UserNT, UserNT(1, "ada")),
        (UserTD, {"id": 1, "name": "ada"}),
    ],
)
def test_execute(conn: Any, form: Any, expected: Any) -> None:
    assert execute(conn, SQL[form](QUERY)).fetchall() == [expected]


def test_execute_pydantic(conn: Any) -> None:
    pydantic = pytest.importorskip("pydantic")

    class User(pydantic.BaseModel):
        id: int
        name: str

    assert execute(conn, SQL[User](QUERY)).fetchone() == User(id=1, name="ada")


def test_execute_protocol(conn: Any) -> None:
    row: Named | None = execute(conn, SQL[Named](QUERY)).fetchone()
    assert row is not None
    assert (row.id, row.name) == (1, "ada")


def test_execute_params(conn: Any) -> None:
    query = SQL[tuple[int]]("--sql\nSELECT %(n)s::INTEGER + 1")
    assert execute(conn, query, {"n": 41}).fetchone() == (42,)


def test_execute_without_row_form(conn: Any) -> None:
    assert execute(conn, SQL(QUERY)).fetchall() == [(1, "ada")]


def test_scalar_inferred(conn: Any) -> None:
    query = SQL[tuple[str | None]]("--sql\nSELECT NULLIF(%s, '')::TEXT")
    assert scalar(conn, query, ("ada",)) == "ada"
    assert scalar(conn, query, params=("",)) is None


def test_scalar_form(conn: Any) -> None:
    assert scalar(conn, "--sql\nSELECT 'admin'::TEXT", Literal["admin", "user"]) == "admin"
    assert scalar(conn, "--sql\nSELECT %(n)s::INTEGER", int | None, {"n": 7}) == 7


def test_scalar_ignores_row_factories(conn: Any) -> None:
    conn.row_factory = dict_row
    try:
        assert scalar(conn, SQL[UserDC]("--sql\nSELECT 1::INTEGER"), int) == 1
    finally:
        conn.row_factory = tuple_row


def test_scalar_no_rows(conn: Any) -> None:
    with pytest.raises(LookupError, match="no rows"):
        scalar(conn, SQL[tuple[int]]("--sql\nSELECT 1::INTEGER WHERE false"))


def test_scalar_many_columns(conn: Any) -> None:
    with pytest.raises(ValueError, match="too many values"):
        scalar(conn, QUERY, tuple[int, str])


def test_row_form_template() -> None:
    user_id = 1
    query = SQLTemplate[UserTD](
        t"--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = {user_id}"
    )
    assert row_form(query) is UserTD
    assert query.template.values == (1,)


def test_execute_template(conn: Any) -> None:
    if tuple(int(part) for part in psycopg.__version__.split(".")[:2]) < (3, 3):
        pytest.skip("psycopg runs t-strings from 3.3")
    n = 41
    query = SQLTemplate[UserTD](t"--sql\nSELECT {n}::INTEGER + 1 AS id, 'ada'::TEXT AS name")
    assert execute(conn, query).fetchone() == {"id": 42, "name": "ada"}
    assert scalar(conn, SQLTemplate[tuple[int]](t"--sql\nSELECT {n}::INTEGER")) == 41

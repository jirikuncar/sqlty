"""Values the drivers return against the Python types sqlty infers.

``tests/fixtures/driver_values.toml`` holds a value of each column type per dialect.
Each is stored in a one-column table, read back through the dialect's usual driver, and
checked against the Python type sqlty infers for ``SELECT c FROM t``; a NULL read back
must come from a column sqlty says can be NULL. SQLite runs in every test run; PostgreSQL
with ``SQLTY_PG_DSN`` (the pg-fidelity job), MySQL with ``SQLTY_MYSQL_DSN``
(``mysql://user:password@host:port/database`` or ``unix_socket=...``, read by PyMySQL).

A column sqlty leaves untyped is not checked; ``KNOWN_GAPS`` lists understood mismatches.
"""

from __future__ import annotations

import os
import sqlite3
import tomllib
from collections.abc import Callable, Iterator
from contextlib import closing
from pathlib import Path
from typing import Any

import pytest

from sqlty import core
from tests._value_types import matches

CASES: dict[str, dict[str, str]] = tomllib.loads(
    (Path(__file__).parent / "fixtures" / "driver_values.toml").read_text()
)

# Understood mismatches, by "dialect: type", with the reason.
KNOWN_GAPS: dict[str, str] = {
    "sqlite: BOOLEAN": "sqlite3 returns int (#401)",
    "sqlite: DATE": "sqlite3 returns str without detect_types (#401)",
    "sqlite: TIMESTAMP": "sqlite3 returns str without detect_types (#401)",
    "sqlite: DATETIME": "sqlite3 returns str (#401)",
    "sqlite: FLOATING POINT": "SQLite gives it INTEGER affinity, which keeps a non-integral value REAL",
}

Fetch = Callable[[str, str], list[Any]]


def _check(dialect: str, sql_type: str, value_sql: str, fetch: Fetch) -> None:
    schema = f"CREATE TABLE t (c {sql_type});"
    [column] = core.analyze_select("SELECT c FROM t", dialect=dialect, schema_sql=schema) or [None]
    assert column is not None
    values = fetch(schema, value_sql)
    python_type = column.python_type
    problems = (
        []
        if python_type is None
        else [
            f"{value!r} ({type(value).__name__}) is not {python_type}"
            for value in values
            if not matches(value, python_type)
        ]
    )
    if f"{dialect}: {sql_type}" in KNOWN_GAPS:
        assert problems, f"{dialect}: {sql_type} matches now: remove it from KNOWN_GAPS"
        pytest.xfail(KNOWN_GAPS[f"{dialect}: {sql_type}"])
    assert not problems, f"{dialect} {sql_type}: " + "; ".join(problems)
    assert python_type is None or python_type.endswith("| None") or python_type == "typing.Any"


@pytest.mark.parametrize(("sql_type", "value_sql"), CASES["sqlite"].items())
def test_sqlite3(sql_type: str, value_sql: str) -> None:
    def fetch(schema: str, value: str) -> list[Any]:
        with closing(sqlite3.connect(":memory:")) as conn:
            conn.execute(schema)
            conn.execute(f"INSERT INTO t VALUES ({value}), (NULL)")
            return [row[0] for row in conn.execute("SELECT c FROM t")]

    _check("sqlite", sql_type, value_sql, fetch)


@pytest.fixture(scope="module")
def pg() -> Iterator[Any]:
    dsn = os.environ.get("SQLTY_PG_DSN")
    if not dsn:
        pytest.skip("SQLTY_PG_DSN not set")
    psycopg = pytest.importorskip("psycopg")
    with psycopg.connect(dsn, autocommit=True) as conn:
        yield conn


@pytest.mark.pg
@pytest.mark.parametrize(("sql_type", "value_sql"), CASES["postgres"].items())
def test_psycopg(pg: Any, sql_type: str, value_sql: str) -> None:
    def fetch(schema: str, value: str) -> list[Any]:
        pg.execute("DROP TABLE IF EXISTS pg_temp.t")
        pg.execute(schema.replace("CREATE TABLE", "CREATE TEMP TABLE"))
        pg.execute(f"INSERT INTO t VALUES ({value}), (NULL)")
        # Each case recreates `t` with another type, so the query is not prepared.
        return [row[0] for row in pg.execute("SELECT c FROM t", prepare=False).fetchall()]

    _check("postgres", sql_type, value_sql, fetch)


@pytest.fixture(scope="module")
def mysql() -> Iterator[Any]:
    dsn = os.environ.get("SQLTY_MYSQL_DSN")
    if not dsn:
        pytest.skip("SQLTY_MYSQL_DSN not set")
    pymysql = pytest.importorskip("pymysql")
    from urllib.parse import parse_qs, urlparse

    url = urlparse(dsn)
    options = {key: values[0] for key, values in parse_qs(url.query).items()}
    conn = pymysql.connect(
        host=url.hostname or "localhost",
        port=url.port or 3306,
        user=url.username or "root",
        password=url.password or "",
        database=url.path.lstrip("/"),
        autocommit=True,
        **options,
    )
    with closing(conn):
        yield conn


@pytest.mark.parametrize(("sql_type", "value_sql"), CASES["mysql"].items())
def test_pymysql(mysql: Any, sql_type: str, value_sql: str) -> None:
    def fetch(schema: str, value: str) -> list[Any]:
        with mysql.cursor() as cursor:
            cursor.execute("DROP TEMPORARY TABLE IF EXISTS t")
            cursor.execute(schema.replace("CREATE TABLE", "CREATE TEMPORARY TABLE"))
            cursor.execute(f"INSERT INTO t VALUES ({value}), (NULL)")
            cursor.execute("SELECT c FROM t")
            return [row[0] for row in cursor.fetchall()]

    _check("mysql", sql_type, value_sql, fetch)

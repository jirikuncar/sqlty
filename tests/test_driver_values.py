"""Values the drivers return against the Python types sqlty infers.

``tests/fixtures/driver_values.toml`` holds a value of each column type per dialect.
Each is stored in a one-column table, read back through the dialect's usual driver, and
checked against the Python type sqlty infers for ``SELECT c FROM t``; a NULL read back
must come from a column sqlty says can be NULL. SQLite runs in every test run; PostgreSQL
with ``SQLTY_PG_DSN`` (the pg-fidelity job), MySQL with ``SQLTY_MYSQL_DSN``
(``mysql://user:password@host:port/database`` or ``unix_socket=...``, read by PyMySQL).

A column sqlty leaves untyped is not checked; ``KNOWN_GAPS`` lists understood mismatches.

``PARAM_TEXTS`` are passed as ``str`` arguments for parameters of each type: the server
must reject every text ``param-type-mismatch`` reports (``pg_input_is_valid``).
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
from inline_snapshot import snapshot

from sqlty import core
from tests._value_types import matches

CASES: dict[str, dict[str, str]] = tomllib.loads(
    (Path(__file__).parent / "fixtures" / "driver_values.toml").read_text()
)

# Understood mismatches, by "dialect: type", with the reason.
KNOWN_GAPS: dict[str, str] = {
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


PARAM_TYPES = {
    "smallint": "SMALLINT",
    "integer": "INTEGER",
    "bigint": "BIGINT",
    "numeric": "NUMERIC(10, 2)",
    "real": "REAL",
    "double precision": "DOUBLE PRECISION",
    "boolean": "BOOLEAN",
    "uuid": "UUID",
    "date": "DATE",
    "time": "TIME",
    "timetz": "TIMETZ",
    "timestamp": "TIMESTAMP",
    "timestamptz": "TIMESTAMPTZ",
    "interval": "INTERVAL",
}

# fmt: off
PARAM_TEXTS = [
    "", " ", "abc", "1", " 42 ", "-7", "0x1F", "0o17", "0b101", "1_000", "_1", "1__0", "70000",
    "2147483648", "9223372036854775808", "1.5", ".5", "5.", "1e3", "1_0.5", "1,50", "NaN",
    "-Infinity", "inf", "0x1p3", "t", "yes", "o", "of", "off", "on", "nope",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11", "{A0EEBC99-9C0B4EF8-BB6D6BB9-BD380A11}",
    "a0eebc99", "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11-", "2026-11-26", "2024-02-29",
    "2026-02-29", "2026-13-01", "2026-11-26 07:00", "2026-11-26T07:00:00.5",
    "2026-11-26 24:00:00", "2026-11-26 25:00", "23:59:60", "24:00", "12:60", "25:00",
    "allballs", "now", "today", "yesterday", "epoch", "infinity", "January 8, 99 BC",
    "not a date", "Japan", "1 day 2 hours ago", "@ 1 minute", "P1Y2M", "PT", ".", "forever",
    "30:00.5",
    "abcé",
]
# fmt: on


def reported_texts(sql_type: str) -> set[str]:
    calls = "\n".join(
        f"conn.execute('SELECT 1 FROM t WHERE c = %s', ({text!r},))" for text in PARAM_TEXTS
    )
    source = f"import psycopg\n\nconn = psycopg.connect('')\n{calls}\n"
    settings = core.Settings(schema_sql=f"CREATE TABLE t (c {sql_type});")
    lines = {
        d["line"] for d in core.check_source(source, settings) if d["rule"] == "param-type-mismatch"
    }
    return {text for line, text in enumerate(PARAM_TEXTS, start=4) if line in lines}


@pytest.mark.pg
@pytest.mark.parametrize("name", PARAM_TYPES)
def test_reported_text_is_rejected_by_postgres(pg: Any, name: str) -> None:
    found = reported_texts(PARAM_TYPES[name])
    accepted = {
        text
        for text in found
        if pg.execute("SELECT pg_input_is_valid(%s, %s)", (text, name)).fetchone()[0]
    }
    assert not accepted


def test_reported_text_counts() -> None:
    assert {
        name: len(reported_texts(sql_type)) for name, sql_type in PARAM_TYPES.items()
    } == snapshot(
        {
            "smallint": 58,
            "integer": 57,
            "bigint": 56,
            "numeric": 46,
            "real": 49,
            "double precision": 49,
            "boolean": 59,
            "uuid": 63,
            "date": 20,
            "time": 20,
            "timetz": 18,
            "timestamp": 21,
            "timestamptz": 21,
            "interval": 21,
        }
    )

"""Rows from sqlglot's expression typing tests, for the dialects sqlty specifies.

`tests/fixtures/optimizer/annotate_functions.sql` and `annotate_types.sql` pair an
expression over a table `tbl` with the type sqlglot infers. The expected types are
sqlglot's, not the database's, so with `SQLTY_PG_DSN` or `SQLTY_MYSQL_DSN` set each
PostgreSQL or MySQL row runs on the server instead, and the type of the value the
reference driver returns is the expected one; sqlglot's type stays in the provenance. A
row the server rejects is not comparable. Without a server, the expected SQL type is
mapped to Python the way sqlty maps it.
"""

from __future__ import annotations

import re
import urllib.request
from collections.abc import Callable, Iterator
from functools import cache
from pathlib import Path

from harness import Row

URL = "https://raw.githubusercontent.com/tobymao/sqlglot/v{version}/tests/fixtures/optimizer/{name}"
CACHE = Path(__file__).parent / ".cache"
# sqlglot dialect -> sqlty dialect; "" is sqlglot's generic dialect.
DIALECTS = {"postgres": "postgres", "mysql": "mysql", "": "generic"}
# The columns of sqlglot's `tbl`, spelled per sqlty dialect.
COLUMNS = {
    "bin_col": {"postgres": "BYTEA", "mysql": "BLOB", "generic": "BINARY"},
    "str_col": {"postgres": "TEXT", "mysql": "TEXT", "generic": "TEXT"},
    "date_col": "DATE",
    "float_col": {"postgres": "REAL", "mysql": "FLOAT", "generic": "FLOAT"},
    "timestamp_col": "TIMESTAMP",
    "double_col": "DOUBLE PRECISION",
    "bigint_col": "BIGINT",
    "smallint_col": "SMALLINT",
    "int_col": "INT",
    "bool_col": "BOOLEAN",
    "interval_col": {"postgres": "INTERVAL", "generic": "INTERVAL"},
    "obj_col": {"postgres": "JSONB", "mysql": "JSON", "generic": "JSON"},
    "array_col": {"postgres": "TEXT[]", "generic": "TEXT[]"},
    "bit_col": {"postgres": "BIT(3)", "mysql": "BIT(3)", "generic": "BIT(3)"},
}


# A value of each column, for the row the server reads.
VALUES = {
    "postgres": "('\\x616263', 'abc', '2024-01-02', 1.5, '2024-01-02 03:04:05', 2.5, 3, 4, 5, TRUE, "
    "'1 day', '{\"a\": 1}', '{a,b}', B'101')",
    "mysql": "(x'616263', 'abc', '2024-01-02', 1.5, '2024-01-02 03:04:05', 2.5, 3, 4, 5, TRUE, '{\"a\": 1}', B'101')",
}


def rows() -> Iterator[Row]:
    from importlib.metadata import version as installed

    from sqlty import core

    version = installed("sqlglot")

    servers = {"postgres": _postgres(), "mysql": _mysql()}
    for name in ("annotate_functions.sql", "annotate_types.sql"):
        for dialect, sql, expected in _pairs(_fixture(name, version)):
            schema = _schema(dialect)
            provenance = f"{name}: sqlglot {expected}"
            if (run := servers.get(dialect)) is not None:
                observed = run(sql)
                provenance += f", server {observed}"
                # A NULL or an error tells nothing of the type.
                expected_python = (
                    None if observed.startswith("error") or observed == "None" else observed
                )
            else:
                python = core.analyze_select(f"SELECT CAST(x AS {expected}) AS c", dialect=dialect)
                expected_python = python[0].python_type if python else None
            yield Row(
                "sqlglot", version, dialect, "expression", sql, expected_python, provenance, schema
            )


def _name(value: object) -> str:
    cls = type(value)
    return cls.__name__ if cls.__module__ == "builtins" else f"{cls.__module__}.{cls.__qualname__}"


def _postgres() -> Callable[[str], str] | None:
    import os

    if not (dsn := os.environ.get("SQLTY_PG_DSN")):
        return None
    import psycopg

    conn = psycopg.connect(dsn, autocommit=True)
    conn.execute("DROP TABLE IF EXISTS pg_temp.tbl")
    conn.execute(_schema("postgres").replace("CREATE TABLE", "CREATE TEMP TABLE"))
    conn.execute(f"INSERT INTO tbl VALUES {VALUES['postgres']}")

    def run(sql: str) -> str:
        try:
            value = conn.execute(f"SELECT {sql} AS x FROM tbl", prepare=False).fetchone()[0]
        except psycopg.Error as exc:
            return f"error {type(exc).__name__}"
        return "None" if value is None else _name(value)

    return run


def _mysql() -> Callable[[str], str] | None:
    import os
    from urllib.parse import parse_qs, urlparse

    if not (dsn := os.environ.get("SQLTY_MYSQL_DSN")):
        return None
    import pymysql

    url = urlparse(dsn)
    options = {key: values[0] for key, values in parse_qs(url.query).items()}
    conn = pymysql.connect(
        host=url.hostname or "localhost",
        user=url.username or "root",
        password=url.password or "",
        database=url.path.lstrip("/"),
        autocommit=True,
        **options,
    )
    cursor = conn.cursor()
    cursor.execute("DROP TEMPORARY TABLE IF EXISTS tbl")
    cursor.execute(_schema("mysql").replace("CREATE TABLE", "CREATE TEMPORARY TABLE"))
    cursor.execute(f"INSERT INTO tbl VALUES {VALUES['mysql']}")

    def run(sql: str) -> str:
        try:
            cursor.execute(f"SELECT {sql} AS x FROM tbl")
            value = cursor.fetchone()[0]
        except pymysql.Error as exc:
            return f"error {type(exc).__name__}"
        return "None" if value is None else _name(value)

    return run


def _fixture(name: str, version: str) -> str:
    path = CACHE / f"{version}-{name}"
    if not path.exists():
        CACHE.mkdir(exist_ok=True)
        with urllib.request.urlopen(URL.format(version=version, name=name)) as response:  # noqa: S310 - a pinned GitHub URL
            path.write_bytes(response.read())
    return path.read_text()


def _pairs(text: str) -> Iterator[tuple[str, str, str]]:
    """`(sqlty dialect, expression, expected type)` of each pair for a dialect sqlty specifies."""
    for block in re.split(r"\n\s*\n", text):
        lines = [line for line in block.strip().splitlines() if not line.startswith("--")]
        meta = [line for line in lines if line.startswith("#")]
        body = [line.rstrip(";") for line in lines if not line.startswith("#")]
        if len(body) != 2:
            continue
        dialects = [""]
        for line in meta:
            if match := re.match(r"#\s*dialect:\s*(.*)", line):
                dialects = [d.strip() for d in match.group(1).split(",")]
        for dialect in dialects:
            if dialect in DIALECTS:
                yield DIALECTS[dialect], body[0], body[1]


@cache
def _schema(dialect: str) -> str:
    columns = []
    for name, spelling in COLUMNS.items():
        ty = spelling if isinstance(spelling, str) else spelling.get(dialect)
        if ty is not None:
            columns.append(f"{name} {ty}")
    return f"CREATE TABLE tbl ({', '.join(columns)});"

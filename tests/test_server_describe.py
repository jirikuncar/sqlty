"""Column names, types and nullability against SQLite and a live MySQL server.

Every query of the MySQL and SQLite mdtests and docs that runs against an explicit
schema is run on the database, and its result columns are compared with
``core.analyze_select``.

- SQLite runs in every test run, on an in-memory database. Names come from
  ``cursor.description``; the declared type of each column of a ``SELECT``, from a
  temporary view.
- MySQL runs when ``SQLTY_MYSQL_DSN`` points to a disposable server (see
  ``tests/test_driver_values.py``). The database ``sqlty_describe`` is dropped and
  created again for each schema. Names come from ``cursor.description``; types and
  nullability, from ``CREATE TEMPORARY TABLE ... AS`` the query. MySQL's nullability is
  only reported, by ``scripts/compat/mysql_nullability.py``: the server says nullable for
  many expressions sqlty proves non-null.

What is compared:

- Names must be equal.
- The Python type must be one the server's type maps to: a SQLite column declared
  ``INTEGER`` is ``int``, a MySQL ``varbinary`` is ``bytes``. Columns sqlty leaves
  untyped or types with ``typing.Any``, sections with type overrides, and server types
  without one Python type (SQLite's ``NUMERIC`` affinity, MySQL's ``json``) are not.
- Queries the database rejects are skipped. ``KNOWN_DIFFERENCES`` lists understood
  mismatches.
"""

from __future__ import annotations

import os
import re
import sqlite3
from collections.abc import Iterator
from contextlib import closing
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

from sqlty import core
from tests._mdtest.parser import parse

ROOT = Path(__file__).parent.parent
SOURCES = [ROOT / "tests" / "mdtest", ROOT / "docs"]

# Understood mismatches, by "<dialect>:<file>:<line>" of the query, with the reason.
KNOWN_DIFFERENCES: dict[str, str] = {
    "mysql:tests/mdtest/mysql/functions.md:217": "MySQL predicates are `int`; sqlty types `MEMBER OF` like `IN`",
    "mysql:tests/mdtest/mysql/functions.md:393": "MySQL predicates are `int`; sqlty types `SOUNDS LIKE` like `=`",
}

# MySQL column type, without its length or attributes -> Python types sqlty may give it
MYSQL_TYPES = {
    "tinyint": {"int", "bool"},
    "smallint": {"int"},
    "mediumint": {"int"},
    "int": {"int"},
    "bigint": {"int"},
    "year": {"int"},
    "decimal": {"decimal.Decimal"},
    "float": {"float"},
    "double": {"float"},
    "char": {"str"},
    "varchar": {"str"},
    "tinytext": {"str"},
    "text": {"str"},
    "mediumtext": {"str"},
    "longtext": {"str"},
    "enum": {"str"},
    "set": {"str"},
    "binary": {"bytes"},
    "varbinary": {"bytes"},
    "tinyblob": {"bytes"},
    "blob": {"bytes"},
    "mediumblob": {"bytes"},
    "longblob": {"bytes"},
    "date": {"datetime.date"},
    "datetime": {"datetime.datetime"},
    "timestamp": {"datetime.datetime"},
    "time": {"datetime.timedelta"},
}


@dataclass(frozen=True)
class Case:
    id: str
    schema_sql: str
    query: str


def corpus(dialect: str) -> list[Case]:
    """The queries of `dialect`'s sections that have a schema and no type overrides."""
    cases = []
    for path in sorted(p for source in SOURCES for p in source.rglob("*.md")):
        for section in parse(path):
            settings = section.settings
            if (
                section.schema_sql is None
                or settings.get("dialect") != dialect
                or "types" in settings
                or "skip" in section.directives
            ):
                continue
            for unit in section.units:
                if unit.lang == "sql" and any(
                    a.kind in {"columns", "inferred"} for a in unit.assertions
                ):
                    case_id = f"{path.relative_to(ROOT)}:{unit.lines[0][0]}"
                    cases.append(Case(case_id, section.schema_sql, unit.code.text))
    return cases


def python_type(column: core.ColumnInfo) -> str | None:
    """The Python type of a column without ``| None``; a literal is the type of its values."""
    python = (column.python_type or "").removesuffix(" | None")
    if python.startswith("typing.Literal["):
        value = python.removeprefix("typing.Literal[")
        return "str" if value[0] in "'\"" else "bool" if value[0] in "TF" else "int"
    return None if python in {"", "typing.Any"} else python


def compare(ours: list[core.ColumnInfo], server: list[tuple[str, set[str] | None]]) -> list[str]:
    """Why sqlty's columns differ from the server's ``(name, Python types)``."""
    if len(ours) != len(server):
        return [f"{len(ours)} columns, the server says {len(server)}"]
    problems = []
    for column, (name, expected) in zip(ours, server, strict=True):
        if column.name != name:
            problems.append(f"name {column.name!r}, the server says {name!r}")
        python = python_type(column)
        if expected is not None and python is not None and python not in expected:
            problems.append(f"column {name!r}: {python}, the server says one of {sorted(expected)}")
    return problems


def check(case: Case, dialect: str, problems: list[str]) -> None:
    if not problems:
        return
    if (why := KNOWN_DIFFERENCES.get(f"{dialect}:{case.id}")) is not None:
        pytest.skip(f"known difference: {why}")
    pytest.fail(f"{case.query}\n  " + "\n  ".join(problems))


def sqlite_types(declared: str) -> set[str] | None:
    """The Python types of a declared SQLite type, by its affinity."""
    declared = declared.upper()
    if "INT" in declared:
        return {"int"}
    if any(name in declared for name in ("CHAR", "CLOB", "TEXT")):
        return {"str"}
    if any(name in declared for name in ("REAL", "FLOA", "DOUB")):
        return {"float"}
    return None


def describe_sqlite(case: Case) -> list[tuple[str, set[str] | None]]:
    with closing(sqlite3.connect(":memory:")) as conn:
        # A recursive CTE without a bound would run forever.
        conn.set_progress_handler(lambda: 1, 1_000_000)
        try:
            conn.executescript(case.schema_sql)
            names = [column[0] for column in conn.execute(case.query).description or []]
        except sqlite3.Error as error:
            pytest.skip(f"SQLite rejects the query: {error}")
        try:
            conn.execute(f"CREATE TEMP VIEW sqlty_describe AS {case.query}")
        except sqlite3.Error:
            return [(name, None) for name in names]
        declared = [row[2] for row in conn.execute("PRAGMA temp.table_info(sqlty_describe)")]
    return [(name, sqlite_types(ty)) for name, ty in zip(names, declared, strict=True)]


@pytest.mark.parametrize("case", corpus("sqlite"), ids=lambda case: case.id)
def test_matches_sqlite(case: Case) -> None:
    ours = core.analyze_select(case.query, dialect="sqlite", schema_sql=case.schema_sql)
    if not ours:
        pytest.skip("sqlty does not type the query")
    check(case, "sqlite", compare(ours, describe_sqlite(case)))


class MySQL:
    """A connection that loads each distinct schema once, into a fresh database."""

    def __init__(self, conn: Any) -> None:
        self.conn = conn
        self.loaded: str | None = None
        self.rejected: str | None = None

    def load(self, schema_sql: str) -> None:
        if schema_sql != self.loaded:
            self.loaded, self.rejected = schema_sql, None
            with self.conn.cursor() as cursor:
                cursor.execute("DROP DATABASE IF EXISTS sqlty_describe")
                cursor.execute("CREATE DATABASE sqlty_describe")
                cursor.execute("USE sqlty_describe")
                for statement in re.split(r";[ \t]*\n", schema_sql):
                    if statement.strip():
                        try:
                            cursor.execute(statement)
                        except self.conn.Error as error:
                            self.rejected = str(error)
        if self.rejected is not None:
            pytest.skip(f"MySQL rejects the schema: {self.rejected}")

    def describe(self, query: str) -> list[tuple[str, str | None, bool | None]]:
        """``(name, type, nullable)`` of each column of `query`; nothing is written. The
        type and nullability are ``None`` when MySQL cannot make a table of the result."""
        with self.conn.cursor() as cursor:
            try:
                self.conn.begin()
                cursor.execute(query)
                names = [column[0] for column in cursor.description or []]
            except self.conn.Error as error:
                pytest.skip(f"MySQL rejects the query: {error}")
            finally:
                self.conn.rollback()
            try:
                cursor.execute("DROP TEMPORARY TABLE IF EXISTS sqlty_result")
                cursor.execute(f"CREATE TEMPORARY TABLE sqlty_result AS {query}")
                cursor.execute("SHOW COLUMNS FROM sqlty_result")
                columns = cursor.fetchall()
            except self.conn.Error:
                return [(name, None, None) for name in names]
        return [
            (name, column[1], column[2] == "YES")
            for name, column in zip(names, columns, strict=True)
        ]


def mysql_types(server_type: str | None) -> set[str] | None:
    return MYSQL_TYPES.get(re.split(r"[ (]", server_type or "", maxsplit=1)[0])


@pytest.fixture(scope="module")
def mysql() -> Iterator[MySQL]:
    dsn = os.environ.get("SQLTY_MYSQL_DSN")
    if not dsn:
        pytest.skip("SQLTY_MYSQL_DSN not set")
    pymysql = pytest.importorskip("pymysql")
    with closing(connect_mysql(pymysql, dsn)) as conn:
        yield MySQL(conn)


def connect_mysql(pymysql: Any, dsn: str) -> Any:
    from urllib.parse import parse_qs, urlparse

    url = urlparse(dsn)
    options = {key: values[0] for key, values in parse_qs(url.query).items()}
    return pymysql.connect(
        host=url.hostname or "localhost",
        port=url.port or 3306,
        user=url.username or "root",
        password=url.password or "",
        autocommit=True,
        **options,
    )


@pytest.mark.parametrize("case", corpus("mysql"), ids=lambda case: case.id)
def test_matches_mysql(case: Case, mysql: MySQL) -> None:
    ours = core.analyze_select(case.query, dialect="mysql", schema_sql=case.schema_sql)
    if not ours:
        pytest.skip("sqlty does not type the query")
    mysql.load(case.schema_sql)
    server = [(name, mysql_types(ty)) for name, ty, _ in mysql.describe(case.query)]
    check(case, "mysql", compare(ours, server))


def test_corpus_is_not_empty() -> None:
    assert len(corpus("sqlite")) > 50
    assert len(corpus("mysql")) > 50

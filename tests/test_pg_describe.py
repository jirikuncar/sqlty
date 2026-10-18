"""Column names, types and nullability against a live PostgreSQL server.

Every query of the PostgreSQL mdtests that runs against an explicit schema is prepared
on the server, and the names and types of its result columns, as the server describes
them, are compared with ``core.analyze_select``. Every PostgreSQL query of the mdtests
and docs with a schema then runs on rows ``tests/_pg_seed.py`` inserts, in a transaction
rolled back after it, and a NULL in a column sqlty types as non-null fails. The job runs
when ``SQLTY_PG_DSN`` points to a disposable database::

    SQLTY_PG_DSN=postgresql://postgres:postgres@localhost:5432/postgres \\
        uv run pytest tests/test_pg_describe.py -rs

The schema of each mdtest section is loaded into an empty database first; every
non-system schema is dropped between sections, so never point it at real data.

What is compared, and what is tolerated:

- Names must be equal.
- A type must be equal after normalisation: ``DECIMAL(10, 2)`` is ``numeric`` (the
  modifier is ignored), ``ARRAY<INT>`` is ``int4[]``. A column that sqlty types only by
  its Python type (a literal) must have a server type of that family.
- A column sqlty leaves untyped, and a server type that is a domain, enum or composite,
  are not compared.
- Queries the server rejects (placeholders in sqlty's ``%s`` or ``:name`` style, a
  schema using an unavailable extension) are skipped, with the reason.
- ``KNOWN_DIFFERENCES`` lists mismatches that are understood and not fixed yet.

``scripts/compat/pg_nullability.py`` reports the reverse: columns sqlty types as nullable
that are never NULL on the seeded rows.
"""

from __future__ import annotations

import contextlib
import os
import re
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

from sqlty import core
from tests._mdtest.parser import Section, parse
from tests._pg_seed import seed

DSN = os.environ.get("SQLTY_PG_DSN")
ROOT = Path(__file__).parent.parent
CORPUS = Path(__file__).parent / "mdtest" / "postgres"

needs_server = [
    pytest.mark.pg,
    pytest.mark.skipif(not DSN, reason="SQLTY_PG_DSN not set"),
]

# Understood mismatches, keyed by "<file>:<line>" of the query in its mdtest (or a
# prefix of it, such as "returning.md"), with the reason. A listed query is skipped when
# it mismatches; remove its entry once the mismatch is fixed.
KNOWN_DIFFERENCES: dict[str, str] = {}

# sqlty SQL type -> pg_type.typname
SQL_TO_PG = {
    "SMALLINT": "int2",
    "INT": "int4",
    "INTEGER": "int4",
    "BIGINT": "int8",
    "DECIMAL": "numeric",
    "NUMERIC": "numeric",
    "FLOAT": "float4",
    "REAL": "float4",
    "DOUBLE": "float8",
    "TEXT": "text",
    "VARCHAR": "varchar",
    "CHAR": "bpchar",
    "BOOLEAN": "bool",
    "DATE": "date",
    "TIME": "time",
    "TIMETZ": "timetz",
    "TIMESTAMP": "timestamp",
    "TIMESTAMPTZ": "timestamptz",
    "INTERVAL": "interval",
    "UUID": "uuid",
    "JSON": "json",
    "JSONB": "jsonb",
    "BYTEA": "bytea",
    "VARBINARY": "bytea",
    "INET": "inet",
    "CIDR": "cidr",
    "MACADDR": "macaddr",
    "MONEY": "money",
    "VOID": "void",
}

# Python type -> server types a column typed only by it (a literal) may have
PYTHON_TO_PG = {
    "int": {"int2", "int4", "int8"},
    "str": {"text", "varchar", "bpchar", "unknown", "name"},
    "bool": {"bool"},
    "float": {"float4", "float8"},
    "decimal.Decimal": {"numeric"},
    "datetime.date": {"date"},
    "datetime.datetime": {"timestamp", "timestamptz"},
    "datetime.time": {"time", "timetz"},
    "datetime.timedelta": {"interval"},
}


@dataclass(frozen=True)
class Case:
    id: str
    schema_sql: str
    query: str


def _corpus() -> Iterator[Case]:
    for path in sorted(CORPUS.glob("*.md")):
        for section in parse(path):
            yield from _cases(path, section)


def _cases(path: Path, section: Section) -> Iterator[Case]:
    settings = section.settings
    schema_sql = section.schema_sql
    if schema_sql is None or settings.get("dialect", "postgres") != "postgres":
        return
    if "mdtest:skip" in section.directives or "skip" in section.directives:
        return
    for unit in section.units:
        if unit.lang != "sql" or not any(a.kind == "columns" for a in unit.assertions):
            continue
        yield Case(f"{path.name}:{unit.lines[0][0]}", schema_sql, unit.code.text)


@dataclass(frozen=True)
class Queries:
    id: str
    schema_sql: str
    queries: list[tuple[int, str]]


def nullability_corpus() -> Iterator[Queries]:
    """Each section of the mdtests and docs with a PostgreSQL schema, with its typed queries."""
    for path in [
        *sorted((ROOT / "tests" / "mdtest").rglob("*.md")),
        *sorted((ROOT / "docs").rglob("*.md")),
    ]:
        for section in parse(path):
            schema_sql = section.schema_sql
            if (
                schema_sql is None
                or section.settings.get("dialect", "postgres") != "postgres"
                or "skip" in section.directives
            ):
                continue
            queries = [
                (unit.lines[0][0], unit.code.text)
                for unit in section.units
                if unit.lang == "sql"
                and any(a.kind in {"columns", "inferred"} for a in unit.assertions)
            ]
            if queries:
                yield Queries(f"{path.relative_to(ROOT)}:{section.line}", schema_sql, queries)


def non_null(column: core.ColumnInfo) -> bool:
    """Whether sqlty says the column is never NULL; ``typing.Any`` (JSON) includes None."""
    if column.python_type is None:
        return False
    return not {"None", "typing.Any"} & set(column.python_type.split(" | "))


def run(conn: Any, query: str) -> list[tuple[Any, ...]] | None:
    """The rows of a query, in a transaction rolled back after it; ``None`` if it fails."""
    import psycopg

    try:
        with conn.transaction(force_rollback=True):
            cursor = conn.execute(query)
            return cursor.fetchall() if cursor.description else []
    except psycopg.Error:
        return None


def _normalise(sql_type: str) -> str:
    """``DECIMAL(10, 2)`` -> ``numeric``, ``ARRAY<INT>`` -> ``int4[]``."""
    sql_type = sql_type.strip().upper()
    if sql_type.startswith("ARRAY<") and sql_type.endswith(">"):
        return _normalise(sql_type[len("ARRAY<") : -1]) + "[]"
    base = re.split(r"[(<\[]", sql_type, maxsplit=1)[0].strip()
    return SQL_TO_PG.get(base, base.lower())


def compare(
    ours: core.ColumnInfo,
    name: str,
    server_type: str,
    server_kind: str,
    domains: dict[str, str] | None = None,
) -> str | None:
    """Why sqlty's column differs from the server's, or ``None`` when they agree.

    The server describes a domain column by its base type, so ``domains`` maps each
    domain name to that base type.
    """
    if ours.name != name:
        return f"name {ours.name!r}, server says {name!r}"
    if server_kind in {"d", "e", "c"}:  # domain, enum, composite
        return None
    if ours.sql_type is not None:
        expected = _normalise(ours.sql_type)
        expected = (domains or {}).get(expected, expected)
        if expected != server_type:
            return f"column {name!r}: type {ours.sql_type} ({expected}), server says {server_type}"
        return None
    python_type = (ours.python_type or "").removesuffix(" | None")
    families = PYTHON_TO_PG.get(python_type)
    if families is not None and server_type not in families:
        return f"column {name!r}: type {python_type}, server says {server_type}"
    return None


def _known_difference(case_id: str) -> str | None:
    return next((why for key, why in KNOWN_DIFFERENCES.items() if case_id.startswith(key)), None)


@pytest.fixture(scope="module")
def conn() -> Iterator[Any]:
    psycopg = pytest.importorskip("psycopg")
    with psycopg.connect(DSN, autocommit=True) as conn:
        conn.execute("SET statement_timeout = '5s'")
        yield conn


def load_pg_types(conn: Any) -> dict[int, tuple[str, str]]:
    """``oid -> (name, kind)``; an array's name is its element's with ``[]``."""
    rows = conn.execute(
        "SELECT oid, typname, typtype, typelem, typcategory FROM pg_type"
    ).fetchall()
    by_oid = {oid: (name, kind, elem, category) for oid, name, kind, elem, category in rows}
    types: dict[int, tuple[str, str]] = {}
    for oid, (name, kind, elem, category) in by_oid.items():
        if category == "A" and elem in by_oid:
            element_name, element_kind, _, _ = by_oid[elem]
            types[oid] = (element_name + "[]", element_kind)
        else:
            types[oid] = (name, kind)
    return types


@pytest.fixture(scope="module")
def pg_types(conn: Any) -> dict[int, tuple[str, str]]:
    return load_pg_types(conn)


class Schemas:
    """Loads each distinct mdtest schema once, replacing the previous one."""

    def __init__(self, conn: Any) -> None:
        self.conn = conn
        self.loaded: str | None = None
        self.seeded = False

    def load(self, schema_sql: str, *, seeded: bool = False) -> None:
        if schema_sql != self.loaded:
            self.loaded = schema_sql
            self.seeded = False
            self._reset()
            try:
                self.conn.execute(schema_sql)
            except Exception:
                # Run what the server accepts; a query that needs the rest is rejected
                # by the server too, and skipped.
                for statement in re.split(r";[ \t]*\n", schema_sql):
                    if statement.strip():
                        with contextlib.suppress(Exception):
                            self.conn.execute(statement)
        if seeded and not self.seeded:
            self.seeded = True
            seed(self.conn)

    def _reset(self) -> None:
        schemas = self.conn.execute(
            "SELECT nspname FROM pg_namespace "
            "WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema'"
        ).fetchall()
        for (name,) in schemas:
            self.conn.execute(f'DROP SCHEMA IF EXISTS "{name}" CASCADE')
        self.conn.execute("CREATE SCHEMA public")


@pytest.fixture(scope="module")
def schemas(conn: Any) -> Schemas:
    return Schemas(conn)


def load_domains(conn: Any) -> dict[str, str]:
    """``domain name -> base type name`` for the domains the loaded schema defines."""
    rows = conn.execute(
        "SELECT d.typname, b.typname FROM pg_type d JOIN pg_type b ON b.oid = d.typbasetype "
        "WHERE d.typtype = 'd'"
    ).fetchall()
    return dict(rows)


def describe(conn: Any, query: str) -> list[tuple[str, int]]:
    """Names and type OIDs of a query's columns; the query is prepared, not run."""
    from psycopg import pq

    pgconn = conn.pgconn
    result = pgconn.prepare(b"sqlty_describe", query.encode())
    if result.status != pq.ExecStatus.COMMAND_OK:
        message = result.error_message.decode(errors="replace").strip()
        pytest.skip(f"the server rejects the query: {message.splitlines()[0]}")
    try:
        description = pgconn.describe_prepared(b"sqlty_describe")
        return [
            (description.fname(i).decode(), description.ftype(i))
            for i in range(description.nfields)
        ]
    finally:
        pgconn.exec_(b"DEALLOCATE sqlty_describe")


@pytest.mark.parametrize("case", list(_corpus()), ids=lambda case: case.id)
@needs_server[0]
@needs_server[1]
def test_matches_server(
    case: Case, conn: Any, pg_types: dict[int, tuple[str, str]], schemas: Schemas
) -> None:
    schemas.load(case.schema_sql)
    server = describe(conn, case.query)
    ours = core.analyze_select(case.query, dialect="postgres", schema_sql=case.schema_sql)
    if ours is None:
        pytest.skip("sqlty does not type the query")
    if any(oid not in pg_types for _, oid in server):
        # Types the schema just created (enums, domains) have oids the fixture never saw.
        pg_types.update(load_pg_types(conn))
    domains = load_domains(conn)
    problems: list[str] = []
    if len(ours) != len(server):
        problems.append(f"{len(ours)} columns, server says {len(server)}")
    else:
        for column, (name, oid) in zip(ours, server, strict=True):
            server_type, kind = pg_types.get(oid, (f"oid {oid}", "b"))
            if problem := compare(column, name, server_type, kind, domains):
                problems.append(problem)
    if problems:
        if (why := _known_difference(case.id)) is not None:
            pytest.skip(f"known difference: {why}")
        pytest.fail(f"{case.query}\n  " + "\n  ".join(problems))


@pytest.mark.parametrize("section", list(nullability_corpus()), ids=lambda section: section.id)
@needs_server[0]
@needs_server[1]
def test_no_null_in_a_non_null_column(section: Queries, conn: Any, schemas: Schemas) -> None:
    schemas.load(section.schema_sql, seeded=True)
    problems: list[str] = []
    for line, query in section.queries:
        ours = core.analyze_select(query, dialect="postgres", schema_sql=section.schema_sql)
        rows = run(conn, query)
        if not ours or not rows or len(rows[0]) != len(ours):
            continue
        for i, column in enumerate(ours):
            if non_null(column) and any(row[i] is None for row in rows):
                problems.append(
                    f"line {line}: {column.name!r} is {column.python_type}, a row has NULL"
                )
    assert problems == []


def test_corpus_is_not_empty() -> None:
    assert len(list(_corpus())) > 20
    assert len(list(nullability_corpus())) > 100

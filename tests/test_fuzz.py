"""Property-based fuzzing of the analyzer: random schemas and queries, checked for invariants.

Off by default; run it on demand (the `Fuzz` workflow does) with::

    SQLTY_FUZZ=1 uv run pytest tests/test_fuzz.py
    SQLTY_FUZZ=1 SQLTY_FUZZ_EXAMPLES=5000 SQLTY_PG_DSN=postgresql:///db uv run pytest tests/test_fuzz.py

Every generated query must analyze without an error, with one column per select item, and
round-trip through `rewrite` and `strip`. With `SQLTY_PG_DSN`, the server also prepares it
and must agree on column names and types, and running it over rows full of NULLs must never
return NULL for a column sqlty says cannot be NULL. A failure prints the smallest schema and
query Hypothesis could shrink it to.
"""

from __future__ import annotations

import os
from collections.abc import Iterator
from dataclasses import dataclass
from typing import Any

import hypothesis
import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty import core

pytestmark = pytest.mark.skipif(not os.environ.get("SQLTY_FUZZ"), reason="set SQLTY_FUZZ=1 to fuzz")

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))
DSN = os.environ.get("SQLTY_PG_DSN")

# Column types, and a literal of each for COALESCE and VALUES.
TYPES: dict[str, str] = {
    "INTEGER": "1",
    "BIGINT": "2::BIGINT",
    "SMALLINT": "3::SMALLINT",
    "NUMERIC(10, 2)": "1.5",
    "DOUBLE PRECISION": "2.5::DOUBLE PRECISION",
    "REAL": "1.5::REAL",
    "TEXT": "'a'",
    "VARCHAR(20)": "'b'::VARCHAR(20)",
    "BOOLEAN": "TRUE",
    "DATE": "DATE '2024-01-01'",
    "TIMESTAMP": "TIMESTAMP '2024-01-01 00:00:00'",
    "TIMESTAMPTZ": "now()",
    "INTERVAL": "INTERVAL '1 day'",
    "UUID": "gen_random_uuid()",
    "JSONB": "'{}'::JSONB",
    "BYTEA": "'\\x00'::BYTEA",
    "INTEGER[]": "ARRAY[1]",
    "TEXT[]": "ARRAY['a']",
}
NUMERIC = {"INTEGER", "BIGINT", "SMALLINT", "NUMERIC(10, 2)", "DOUBLE PRECISION", "REAL"}
TEXT = {"TEXT", "VARCHAR(20)"}
ORDERED = NUMERIC | TEXT | {"DATE", "TIMESTAMP", "TIMESTAMPTZ", "INTERVAL"}


@dataclass(frozen=True)
class Column:
    name: str
    type: str
    not_null: bool


@dataclass(frozen=True)
class Table:
    name: str
    columns: tuple[Column, ...]

    def ddl(self) -> str:
        cols = ", ".join(
            f"{c.name} {c.type}{' NOT NULL' if c.not_null else ''}" for c in self.columns
        )
        return f"CREATE TABLE {self.name} (id INTEGER PRIMARY KEY, {cols});"


@dataclass(frozen=True)
class Case:
    tables: tuple[Table, ...]
    query: str
    items: int

    @property
    def schema(self) -> str:
        return "\n".join(table.ddl() for table in self.tables)


@st.composite
def tables(draw: st.DrawFn) -> tuple[Table, ...]:
    count = draw(st.integers(1, 2))
    return tuple(
        Table(
            f"t{i}",
            tuple(
                Column(f"c{j}", draw(st.sampled_from(sorted(TYPES))), draw(st.booleans()))
                for j in range(draw(st.integers(1, 4)))
            ),
        )
        for i in range(count)
    )


def _expressions(draw: st.DrawFn, columns: list[tuple[str, Column]], aggregate: bool) -> str:
    alias, column = draw(st.sampled_from(columns))
    ref = f"{alias}.{column.name}"
    kind = column.type
    choices = ["column", "cast_text", "coalesce", "is_null", "case"]
    if kind in NUMERIC:
        choices += ["arith", "abs"]
    if kind in TEXT:
        choices += ["lower", "concat"]
    if aggregate:
        choices = ["count", "count_star"] + (["max", "min"] if kind in ORDERED else [])
        choices += ["sum", "avg"] if kind in NUMERIC else []
    match draw(st.sampled_from(choices)):
        case "column":
            return ref
        case "cast_text":
            return f"{ref}::TEXT"
        case "coalesce":
            return f"COALESCE({ref}, {TYPES[kind]})"
        case "is_null":
            return f"{ref} IS NULL"
        case "case":
            return f"CASE WHEN {ref} IS NOT NULL THEN {ref} END"
        case "arith":
            return f"{ref} + 1"
        case "abs":
            return f"abs({ref})"
        case "lower":
            return f"lower({ref})"
        case "concat":
            return f"{ref} || 'x'"
        case "count":
            return f"count({ref})"
        case "count_star":
            return "count(*)"
        case name:
            return f"{name}({ref})"


@st.composite
def cases(draw: st.DrawFn) -> Case:
    schema = draw(tables())
    left = schema[0]
    sources = [("a", left)]
    from_clause = f"{left.name} a"
    if len(schema) > 1:
        join = draw(st.sampled_from(["JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN", ","]))
        from_clause += (
            f", {schema[1].name} b" if join == "," else f" {join} {schema[1].name} b ON b.id = a.id"
        )
        sources.append(("b", schema[1]))
    columns = [(alias, column) for alias, table in sources for column in table.columns]
    aggregate = draw(st.booleans())
    items = [_expressions(draw, columns, aggregate) for _ in range(draw(st.integers(1, 4)))]
    select = ", ".join(f"{item} AS x{i}" for i, item in enumerate(items))
    query = f"SELECT {select} FROM {from_clause}"
    if draw(st.booleans()):
        alias, column = draw(st.sampled_from(columns))
        query += f" WHERE {alias}.{column.name} IS NOT NULL"
    if draw(st.booleans()) and not aggregate:
        query = f"{query} UNION ALL {query}"
    return Case(schema, query, len(items))


FUZZ = settings(
    max_examples=EXAMPLES,
    deadline=None,
    suppress_health_check=[HealthCheck.too_slow, HealthCheck.data_too_large],
)


@FUZZ
@given(cases())
def test_queries_analyze_with_one_column_per_item(case: Case) -> None:
    columns = core.analyze_select(case.query, schema_sql=case.schema)
    assert columns is not None, case.query
    assert [c.name for c in columns] == [f"x{i}" for i in range(case.items)], case.query


@FUZZ
@given(cases())
def test_rewrite_and_strip_round_trip(case: Case) -> None:
    source = f'q = "--sql\\n{case.query}"\n'
    rewritten = core.rewrite_source(source, schema_sql=case.schema)
    assert core.strip_source(rewritten) == source
    assert core.rewrite_source(rewritten, schema_sql=case.schema) == rewritten
    stale = [
        d for d in core.check_source(rewritten, schema_sql=case.schema) if d["rule"] == "stale-type"
    ]
    assert stale == [], rewritten


@pytest.fixture(scope="module")
def conn() -> Iterator[Any]:
    if not DSN:
        pytest.skip("set SQLTY_PG_DSN to compare with a server")
    psycopg = pytest.importorskip("psycopg")
    with psycopg.connect(DSN, autocommit=True) as conn:
        yield conn


def _load(conn: Any, case: Case, rows: int) -> None:
    for table in case.tables:
        conn.execute(f"DROP TABLE IF EXISTS {table.name} CASCADE")
        conn.execute(table.ddl())
        for i in range(rows):
            # Every nullable column NULL, others a literal of their type.
            values = ", ".join(TYPES[c.type] if c.not_null else "NULL" for c in table.columns)
            conn.execute(f"INSERT INTO {table.name} VALUES ({i}, {values})")


@FUZZ
@given(case=cases(), rows=st.integers(0, 2))
def test_server_agrees(conn: Any, case: Case, rows: int) -> None:
    from tests.test_pg_describe import compare, load_pg_types

    ours = core.analyze_select(case.query, schema_sql=case.schema)
    assert ours is not None
    _load(conn, case, rows)
    psycopg = pytest.importorskip("psycopg")
    try:
        cursor = conn.execute(case.query)
    except psycopg.Error:
        # A combination the server rejects (say `max` of an array) says nothing about sqlty.
        hypothesis.reject()
    types = load_pg_types(conn)
    problems = []
    for column, described in zip(ours, cursor.description, strict=True):
        server_type, kind = types.get(described.type_code, (f"oid {described.type_code}", "b"))
        if problem := compare(column, described.name, server_type, kind):
            problems.append(problem)
    for row in cursor.fetchall():
        for column, value in zip(ours, row, strict=True):
            python_type = column.python_type or "typing.Any"
            if (
                value is None
                and not python_type.endswith(" | None")
                and python_type != "typing.Any"
            ):
                problems.append(f"column {column.name!r} typed {python_type} holds NULL")
    assert not problems, f"{case.schema}\n{case.query}\n  " + "\n  ".join(problems)

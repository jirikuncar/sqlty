"""Benchmarks of the engine through ``sqlty.core``, measured by CodSpeed on every commit.

Each query kind of ``synth.KINDS`` is analyzed against the 400-table schema of the CI
budget, loaded once. The schema load, ``rewrite_source`` and ``check_source`` on a
module of every kind are measured too.

Build the extension in release mode first, then run them as plain pytest:
``uv run pytest benchmarks/ --codspeed -p no:randomly --no-cov``.
"""

from __future__ import annotations

import pytest
import synth
from pytest_codspeed import BenchmarkFixture

from sqlty import core

TABLES = 400
SCHEMA_SQL = synth.schema_sql(TABLES)
SCHEMA = core.load_schema(SCHEMA_SQL)
QUERIES = {
    name: sql.format(t="t0007", p="t0006", v="v0007", f="f0007")
    for name, sql, _fields, _params in synth.KINDS
}
SOURCE = "\n".join(
    f'q_{name}_{n} = "--sql\\n{sql} -- {n}"' for n in range(5) for name, sql in QUERIES.items()
)
REWRITTEN = core.rewrite_source(SOURCE, schema_sql=SCHEMA)


@pytest.mark.parametrize("sql", QUERIES.values(), ids=QUERIES.keys())
def test_analyze_select(benchmark: BenchmarkFixture, sql: str) -> None:
    assert benchmark(core.analyze_select, sql, schema_sql=SCHEMA)


@pytest.mark.parametrize("sql", QUERIES.values(), ids=QUERIES.keys())
def test_analyze_params(benchmark: BenchmarkFixture, sql: str) -> None:
    assert benchmark(core.analyze_params, sql, schema_sql=SCHEMA) is not None


def test_load_schema(benchmark: BenchmarkFixture) -> None:
    benchmark(core.load_schema, SCHEMA_SQL)


def test_rewrite_source(benchmark: BenchmarkFixture) -> None:
    assert benchmark(core.rewrite_source, SOURCE, schema_sql=SCHEMA) == REWRITTEN


def test_check_source(benchmark: BenchmarkFixture) -> None:
    assert benchmark(core.check_source, REWRITTEN, schema_sql=SCHEMA) == []


BRANCHED = (
    "from typing import Annotated, LiteralString\n"
    "from sqlty import SQL\n"
    "def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...\n"
    "def find(" + ", ".join(f"f{i}: bool" for i in range(7)) + ") -> list[tuple[int, str]]:\n"
    '    query = "SELECT id, name FROM app.t0007 WHERE TRUE"\n'
    + "".join(f'    if f{i}:\n        query += " AND score > {i}"\n' for i in range(7))
    + "    return fetch_all(query, tuple[int, str])\n"
)


def test_check_source_query_variants(benchmark: BenchmarkFixture) -> None:
    assert benchmark(core.check_source, BRANCHED, schema_sql=SCHEMA, query_variants=128) == []


# Completion per keystroke must not grow with the schema: each operation is measured at
# 50, 540 and 5000 tables, so CodSpeed shows a regression and the sizes side by side.
COMPLETION_SCHEMAS = {
    tables: core.load_schema(synth.schema_sql(tables)) for tables in (50, 540, 5000)
}
COMPLETIONS = {
    "relation_prefix": 'q = "--sql\\nSELECT * FROM t004"',
    "alias_columns": 'q = "--sql\\nSELECT a. FROM app.t0000 a JOIN app.t0001 b ON b.parent_id = a.id"',
    "columns_in_scope": 'q = "--sql\\nSELECT * FROM app.t0000 a JOIN app.t0001 b ON b.parent_id = a.id WHERE s"',
}
CURSORS = {"relation_prefix": "t004", "alias_columns": "a.", "columns_in_scope": "WHERE s"}


@pytest.mark.parametrize("tables", COMPLETION_SCHEMAS)
@pytest.mark.parametrize("operation", COMPLETIONS)
def test_complete_source(benchmark: BenchmarkFixture, operation: str, tables: int) -> None:
    source = COMPLETIONS[operation]
    column = source.index(CURSORS[operation]) + len(CURSORS[operation]) + 1
    found = benchmark(
        core.complete_source, source, 1, column, schema_sql=COMPLETION_SCHEMAS[tables]
    )
    assert found is not None and found.items


MODULE = "\n".join(f'q{n} = "--sql\\nSELECT id FROM app.t{n % 50:04d}"' for n in range(4000))


def test_query_at_in_a_large_module(benchmark: BenchmarkFixture) -> None:
    offset = MODULE.rindex("SELECT") + 7
    assert benchmark(core.query_at, MODULE, offset) is not None

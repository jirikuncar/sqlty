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

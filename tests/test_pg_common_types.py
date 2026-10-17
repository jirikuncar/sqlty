"""Types of literals, CASE, COALESCE, GREATEST/LEAST, set operations and operators
against the types PostgreSQL gives them.

``tests/fixtures/pg_common_types.toml`` holds each query with the type the server
describes for its column, so the comparison needs no server. With ``SQLTY_PG_DSN`` set,
the fixture itself is checked against the server; ``scripts/refresh_pg_types.py``
regenerates it.

A column sqlty leaves untyped is not a mismatch: it stays unknown rather than guessed.
"""

from __future__ import annotations

import os
import tomllib
from pathlib import Path

import pytest

from sqlty import core
from tests.test_pg_describe import compare

FIXTURE = Path(__file__).parent / "fixtures" / "pg_common_types.toml"
CASES = [
    pytest.param(query, server_type, id=f"{section}: {query.removeprefix('SELECT ')}")
    for section, queries in tomllib.loads(FIXTURE.read_text()).items()
    for query, server_type in queries.items()
]

# Mismatches that are understood and not fixed yet, with the reason; listed in
# docs/reference/engine.md#known-gaps.
KNOWN_GAPS: dict[str, str] = {}


@pytest.mark.parametrize(("query", "server_type"), CASES)
def test_matches_postgres(query: str, server_type: str) -> None:
    columns = core.analyze_select(query, dialect="postgres")
    assert columns is not None and len(columns) == 1, query
    column = columns[0]._replace(python_type=_base(columns[0].python_type))
    problem = compare(column, column.name, server_type, "b")
    if query in KNOWN_GAPS:
        assert problem is not None, f"{query} matches PostgreSQL now: remove it from KNOWN_GAPS"
        pytest.xfail(KNOWN_GAPS[query])
    assert problem is None, f"{query}: {problem}"


def _base(python_type: str | None) -> str | None:
    """``typing.Literal['a'] | None`` -> ``str``: the Python type of a value set."""
    if python_type is None or not python_type.startswith("typing.Literal["):
        return python_type
    value = python_type.removeprefix("typing.Literal[")
    if value.startswith(("'", '"')):
        return "str"
    return "bool" if value.startswith(("True", "False")) else "int"


@pytest.mark.pg
@pytest.mark.skipif(not os.environ.get("SQLTY_PG_DSN"), reason="SQLTY_PG_DSN not set")
def test_fixture_is_current() -> None:
    psycopg = pytest.importorskip("psycopg")
    from scripts.refresh_pg_types import describe, type_names

    stale: list[str] = []
    with psycopg.connect(os.environ["SQLTY_PG_DSN"], autocommit=True) as conn:
        typnames = type_names(conn)
        for case in CASES:
            query, server_type = case.values
            actual = typnames[describe(conn, str(query))]
            if actual != server_type:
                stale.append(f"{query}: {server_type} in the fixture, server says {actual}")
    assert not stale, "run scripts/refresh_pg_types.py:\n" + "\n".join(stale)

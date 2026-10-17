"""Types sqlty infers in the ``duckdb`` dialect against DuckDB itself.

DuckDB runs in-process, so this runs in every test run. Each query runs on a table with
one row of values: the type DuckDB describes for a column must map to the Python type
sqlty infers, and the value the Python client returns must be an instance of it. A column
sqlty leaves untyped is not a mismatch; ``KNOWN_GAPS`` lists understood mismatches.
"""

from __future__ import annotations

import re

import pytest

from sqlty import core
from tests._value_types import matches

duckdb = pytest.importorskip("duckdb")

SCHEMA = """
CREATE TABLE t (
    i INTEGER NOT NULL, b BIGINT NOT NULL, h HUGEINT NOT NULL, u UBIGINT NOT NULL,
    s SMALLINT NOT NULL, d DECIMAL(10, 2) NOT NULL, f FLOAT NOT NULL, g DOUBLE NOT NULL,
    v VARCHAR NOT NULL, bl BLOB NOT NULL, bo BOOLEAN NOT NULL, dt DATE NOT NULL,
    tm TIME NOT NULL, ts TIMESTAMP NOT NULL, tz TIMESTAMPTZ NOT NULL, iv INTERVAL NOT NULL,
    uu UUID NOT NULL, j JSON NOT NULL, l INTEGER[] NOT NULL, a INTEGER[3] NOT NULL,
    st STRUCT(a INT, b VARCHAR) NOT NULL, m MAP(VARCHAR, INT) NOT NULL,
    un UNION(n INT, s VARCHAR) NOT NULL, n INTEGER
);
"""
ROW = """
INSERT INTO t VALUES (
    1, 2, 3, 4, 5, 6.5, 1.5, 2.5, 'x', '\\x00'::BLOB, TRUE, DATE '2024-01-02',
    TIME '10:00', TIMESTAMP '2024-01-02 10:00', TIMESTAMPTZ '2024-01-02 10:00:00+00',
    INTERVAL 1 DAY, uuid(), '{"a": 1}', [1, 2], [1, 2, 3], {'a': 1, 'b': 'x'},
    MAP {'k': 1}, union_value(n := 1), NULL
)
"""

# The Python type the DuckDB client returns for each type DuckDB describes, by base name.
PYTHON = {
    "INTEGER": "int",
    "BIGINT": "int",
    "HUGEINT": "int",
    "UBIGINT": "int",
    "SMALLINT": "int",
    "TINYINT": "int",
    "DECIMAL": "decimal.Decimal",
    "FLOAT": "float",
    "DOUBLE": "float",
    "VARCHAR": "str",
    "BLOB": "bytes",
    "BOOLEAN": "bool",
    "DATE": "datetime.date",
    "TIME": "datetime.time",
    "TIMESTAMP": "datetime.datetime",
    "TIMESTAMP WITH TIME ZONE": "datetime.datetime",
    "INTERVAL": "datetime.timedelta",
    "UUID": "uuid.UUID",
}

QUERIES = [
    "SELECT i, b, h, u, s, d, f, g FROM t",
    # `tz` is left out: the client needs pytz to return a TIMESTAMPTZ.
    "SELECT v, bl, bo, dt, tm, ts, iv, uu FROM t",
    "SELECT l, a, st, m, un FROM t",
    "SELECT i + b AS sum_, i * 2 AS twice, d + 1 AS dplus, g / 2 AS half, -i AS neg FROM t",
    "SELECT count(*) AS n, sum(i) AS total, max(v) AS top, min(dt) AS first FROM t",
    "SELECT lower(v) AS lo, length(v) AS len, v || 'y' AS cat, upper(v) AS up FROM t",
    "SELECT dt + INTERVAL 1 DAY AS later, ts - INTERVAL 1 HOUR AS earlier FROM t",
    "SELECT CASE WHEN bo THEN i ELSE 0 END AS c, coalesce(n, i) AS co, nullif(i, 0) AS ni FROM t",
    "SELECT CAST(i AS VARCHAR) AS vs, CAST(v AS VARCHAR) AS vv, i::BIGINT AS ib FROM t",
    "SELECT l[1] AS first_item, st.a AS field FROM t",
]

# Understood mismatches, by query, with the reason.
KNOWN_GAPS: dict[str, str] = {}


@pytest.fixture(scope="module")
def conn():
    conn = duckdb.connect()
    conn.execute(SCHEMA)
    conn.execute(ROW)
    yield conn
    conn.close()


def _expected(described: str) -> str | None:
    """The Python type of a type DuckDB describes, when the table above maps it."""
    base = re.split(r"[(\[]", described, maxsplit=1)[0].strip()
    if described.endswith("[]"):
        inner = _expected(described[:-2])
        return f"list[{inner}]" if inner else None
    if described.endswith("]"):
        inner = _expected(described[: described.rindex("[")])
        return f"tuple[{inner}, ...]" if inner else None
    return PYTHON.get(base)


@pytest.mark.parametrize("query", QUERIES)
def test_matches_duckdb(conn, query: str) -> None:
    cursor = conn.execute(query)
    row = cursor.fetchone()
    ours = core.analyze_select(query, dialect="duckdb", schema_sql=SCHEMA)
    assert ours is not None and len(ours) == len(cursor.description), query
    problems = []
    for column, (name, described, *_), value in zip(ours, cursor.description, row, strict=True):
        python_type = column.python_type
        if python_type is None:
            continue
        if not matches(value, python_type):
            problems.append(f"{name}: value {value!r} is not {python_type}")
        expected = _expected(str(described))
        if expected is not None and python_type.removesuffix(" | None") != expected:
            problems.append(f"{name}: {python_type}, DuckDB describes {described} ({expected})")
    if problems and query in KNOWN_GAPS:
        pytest.xfail(KNOWN_GAPS[query])
    assert not problems, f"{query}\n  " + "\n  ".join(problems)

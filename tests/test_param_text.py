"""`str` arguments that `param-type-mismatch` reports, against what PostgreSQL rejects.

Each text is passed for a parameter of each type and checked; the server must reject every
text sqlty reports (``pg_input_is_valid``), and a snapshot counts the ones it reports.
"""

from __future__ import annotations

import os
from collections.abc import Iterator
from typing import Any

import psycopg
import pytest
from inline_snapshot import snapshot

from sqlty import core

TYPES = {
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
TEXTS = [
    "", " ", "abc", "1", " 42 ", "-7", "0x1F", "0o17", "0b101", "1_000", "_1", "1__0", "70000",
    "2147483648", "9223372036854775808", "1.5", ".5", "5.", "1e3", "1_0.5", "1,50", "NaN",
    "-Infinity", "inf", "0x1p3", "t", "yes", "o", "of", "off", "on", "nope",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11", "{A0EEBC99-9C0B4EF8-BB6D6BB9-BD380A11}",
    "a0eebc99", "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11-", "2026-11-26", "2024-02-29",
    "2026-02-29", "2026-13-01", "2026-11-26 07:00", "2026-11-26T07:00:00.5",
    "2026-11-26 24:00:00", "2026-11-26 25:00", "23:59:60", "24:00", "12:60", "25:00",
    "allballs", "now", "today", "yesterday", "epoch", "infinity", "January 8, 99 BC",
    "not a date", "Japan", "1 day 2 hours ago", "@ 1 minute", "P1Y2M", "PT", ".", "forever",
]
# fmt: on


@pytest.fixture(scope="module")
def pg() -> Iterator[Any]:
    dsn = os.environ.get("SQLTY_PG_DSN")
    if not dsn:
        pytest.skip("SQLTY_PG_DSN not set")
    with psycopg.connect(dsn, autocommit=True) as conn:
        yield conn


def reported(sql_type: str) -> set[str]:
    calls = "\n".join(
        f"conn.execute('SELECT 1 FROM t WHERE c = %s', ({text!r},))" for text in TEXTS
    )
    source = f"import psycopg\n\nconn = psycopg.connect('')\n{calls}\n"
    settings = core.Settings(schema_sql=f"CREATE TABLE t (c {sql_type});")
    lines = {
        d["line"] for d in core.check_source(source, settings) if d["rule"] == "param-type-mismatch"
    }
    return {text for line, text in enumerate(TEXTS, start=4) if line in lines}


@pytest.mark.pg
@pytest.mark.parametrize("name", TYPES)
def test_reported_text_is_rejected_by_postgres(pg: Any, name: str) -> None:
    found = reported(TYPES[name])
    accepted = {
        text
        for text in found
        if pg.execute("SELECT pg_input_is_valid(%s, %s)", (text, name)).fetchone()[0]
    }
    assert not accepted


def test_reported_text_counts() -> None:
    assert {name: len(reported(sql_type)) for name, sql_type in TYPES.items()} == snapshot(
        {
            "smallint": 56,
            "integer": 55,
            "bigint": 54,
            "numeric": 44,
            "real": 47,
            "double precision": 47,
            "boolean": 57,
            "uuid": 61,
            "date": 19,
            "time": 19,
            "timetz": 17,
            "timestamp": 20,
            "timestamptz": 20,
            "interval": 20,
        }
    )

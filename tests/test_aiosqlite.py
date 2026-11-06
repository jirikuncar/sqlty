"""aiosqlite calls ``sqlty check`` reports, run against aiosqlite.

Each case is one call on an ``aiosqlite.Connection`` or a cursor it opened. The source
holding the call is checked with ``check_source``, then the same source runs against an
in-memory database with the same schema: a call sqlty reports must fail there, and a call
it leaves alone must not.
"""

from __future__ import annotations

import asyncio
import sqlite3
from typing import Any

import pytest

from sqlty import core

aiosqlite = pytest.importorskip("aiosqlite")

SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);"

SOURCE = """\
import aiosqlite


async def run(db: aiosqlite.Connection) -> None:
    cursor = await db.cursor()
    await {call}
"""

# Each call, and the rule sqlty reports for it: None when it is left alone.
CASES: list[tuple[str, str | None]] = [
    ('db.execute("SELECT id FROM users WHERE id = ?", (1,))', None),
    (
        'db.execute("SELECT id FROM users WHERE id = ? AND age = ?", (1,))',
        "param-count-mismatch",
    ),
    ('db.execute("SELECT id FROM users WHERE id = :id", {"id": 1})', None),
    (
        'db.execute("SELECT id FROM users WHERE id = :id", {"ident": 1})',
        "param-count-mismatch",
    ),
    ('db.execute_fetchall("SELECT id FROM users WHERE id = ?")', "param-count-mismatch"),
    ('db.execute_fetchall("SELECT id FROM users WHERE id = ?", 1)', "param-type-mismatch"),
    # A str is a sequence of its characters, as sqlite3 reads it.
    ('db.execute_fetchall("SELECT id FROM users WHERE name = ?", "a")', None),
    ('db.execute_insert("INSERT INTO users (id, name) VALUES (?, ?)", (1, "a"))', None),
    (
        'db.execute_insert("INSERT INTO users (id, name) VALUES (?, ?)", parameters=(1,))',
        "param-count-mismatch",
    ),
    (
        'db.execute_insert("INSERT INTO users (id, name) VALUES (?, ?)", (1, None))',
        "param-type-mismatch",
    ),
    ('db.executemany("INSERT INTO users (id, name) VALUES (?, ?)", [(1, "a"), (2, "b")])', None),
    (
        'db.executemany("INSERT INTO users (id, name) VALUES (?, ?)", [(1, "a"), (2,)])',
        "param-count-mismatch",
    ),
    ('cursor.execute("SELECT id FROM users WHERE id = ?", (1,))', None),
    ('cursor.execute("SELECT id FROM users WHERE id = ?", (1, 2))', "param-count-mismatch"),
    (
        'cursor.executemany("UPDATE users SET age = ? WHERE id = ?", [(1, 1), (2,)])',
        "param-count-mismatch",
    ),
]


def _reported(source: str) -> list[str]:
    diagnostics = core.check_source(source, dialect="sqlite", schema_sql=SCHEMA)

    return [d["rule"] for d in diagnostics if d["rule"].startswith("param-")]


async def _run(source: str) -> None:
    namespace: dict[str, Any] = {}
    exec(compile(source, "<aiosqlite case>", "exec"), namespace)

    async with aiosqlite.connect(":memory:") as db:
        await db.execute(SCHEMA)
        await namespace["run"](db)


@pytest.mark.parametrize(("call", "rule"), CASES)
def test_reported_calls_fail_at_run_time(call: str, rule: str | None) -> None:
    source = SOURCE.format(call=call)

    reported = _reported(source)

    assert reported == ([] if rule is None else [rule])

    if rule is None:
        asyncio.run(_run(source))
    else:
        with pytest.raises(sqlite3.Error):
            asyncio.run(_run(source))

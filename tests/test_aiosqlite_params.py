"""aiosqlite raises on the calls ``param-*`` rules report on, and runs the ones they let through.

Each case runs against an in-memory database through aiosqlite and through
``core.check_source``; skipped without aiosqlite (``uv sync --group aiosqlite``).
"""

from __future__ import annotations

import asyncio

import pytest

from sqlty import core

aiosqlite = pytest.importorskip("aiosqlite")

SCHEMA = "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);"
INSERT = '"INSERT INTO meta (key, value) VALUES (?, ?)"'
SELECT = '"SELECT value FROM meta WHERE key = ?"'
NAMED = '"SELECT value FROM meta WHERE key = :key"'

CASES = {
    "execute": (f'db.execute({INSERT}, ("a", "1"))', None),
    "execute short": (f'db.execute({INSERT}, ("a",))', "param-count-mismatch"),
    "execute long": (f'db.execute({SELECT}, ("a", "b"))', "param-count-mismatch"),
    "execute none": (f"db.execute({SELECT})", "param-count-mismatch"),
    "execute keyword": (f'db.execute({INSERT}, parameters=("a",))', "param-count-mismatch"),
    "execute str": (f'db.execute({SELECT}, "a")', None),
    "execute int": (f"db.execute({SELECT}, 1)", "param-type-mismatch"),
    "execute named": (f'db.execute({NAMED}, {{"key": "a"}})', None),
    "execute named missing": (f'db.execute({NAMED}, {{"name": "a"}})', "param-count-mismatch"),
    "execute_insert": (f'db.execute_insert({INSERT}, ("a", "1"))', None),
    "execute_insert short": (f'db.execute_insert({INSERT}, ("a",))', "param-count-mismatch"),
    "execute_fetchall": (f'db.execute_fetchall({SELECT}, ("a",))', None),
    "execute_fetchall none": (f"db.execute_fetchall({SELECT})", "param-count-mismatch"),
    "executemany": (f'db.executemany({INSERT}, [("a", "1"), ("b", "2")])', None),
    "executemany short row": (
        f'db.executemany({INSERT}, [("a", "1"), ("b",)])',
        "param-count-mismatch",
    ),
    "cursor execute": (f'cur.execute({SELECT}, ("a",))', None),
    "cursor execute none": (f"cur.execute({SELECT})", "param-count-mismatch"),
    "cursor executemany short": (f'cur.executemany({INSERT}, [("a",)])', "param-count-mismatch"),
}


def _source(call: str) -> str:
    return (
        "import aiosqlite\n\n"
        "async def run(db: aiosqlite.Connection) -> None:\n"
        "    cur = await db.cursor()\n"
        f"    await {call}\n"
    )


async def _run(call: str) -> None:
    namespace: dict[str, object] = {}
    exec(_source(call), namespace)
    async with aiosqlite.connect(":memory:") as db:
        await db.execute(SCHEMA)
        await namespace["run"](db)  # type: ignore[operator]


@pytest.mark.parametrize(("call", "rule"), CASES.values(), ids=CASES)
def test_aiosqlite_agrees(call: str, rule: str | None) -> None:
    rules = {
        d["rule"] for d in core.check_source(_source(call), dialect="sqlite", schema_sql=SCHEMA)
    }
    assert rules == ({rule} if rule else set())
    if rule is None:
        asyncio.run(_run(call))
    else:
        with pytest.raises((aiosqlite.ProgrammingError, TypeError)):
            asyncio.run(_run(call))

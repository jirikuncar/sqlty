"""rustmy calls ``sqlty check`` reports, run against MySQL with rustmy.

Each case is one call on a rustmy cursor. The source holding the call is checked with
``check_source``, then the same source runs against ``SQLTY_MYSQL_DSN`` with the same
schema, as a temporary table: a call sqlty reports must fail there, and a call it leaves
alone must not. rustmy comes from the ``rustmy`` dependency group.
"""

from __future__ import annotations

from collections.abc import Iterator
from contextlib import closing
from typing import Any

import pytest
import rustmy

from sqlty import core

SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);"

SOURCE = """\
import rustmy


def run(conn: rustmy.Connection) -> None:
    cur = conn.cursor(prepared={prepared})
    {call}
"""

# Each call, and the rule sqlty reports for it: None when it is left alone.
CASES: list[tuple[str, str | None]] = [
    ('cur.execute("SELECT id FROM users WHERE id = %s", (1,))', None),
    ('cur.execute("SELECT id FROM users WHERE id = %s", 1)', None),
    ('cur.execute("SELECT id FROM users WHERE name = %s", "a")', None),
    (
        'cur.execute("SELECT id FROM users WHERE id = %s AND age = %s", (1,))',
        "param-count-mismatch",
    ),
    ('cur.execute("SELECT id FROM users WHERE id = %s AND age = %s", 1)', "param-count-mismatch"),
    ('cur.execute("SELECT id FROM users WHERE id = %s", (1, 2))', "param-count-mismatch"),
    ('cur.execute("SELECT id FROM users WHERE id = %s")', "param-count-mismatch"),
    ('cur.execute("SELECT id FROM users WHERE id = %s", None)', "param-count-mismatch"),
    ('cur.execute("SELECT id FROM users", ())', None),
    ('cur.execute("SELECT id FROM users", (1,))', "param-count-mismatch"),
    ('cur.execute("SELECT id FROM users WHERE id = %(id)s", {"id": 1})', None),
    ('cur.execute("SELECT id FROM users WHERE id = %(id)s", {"ident": 1})', "param-count-mismatch"),
    ('cur.execute("SELECT id FROM users WHERE id = %(id)s", {"id": 1, "x": 2})', None),
    ('cur.execute("SELECT id FROM users WHERE id = %s", args=(1, 2))', "param-count-mismatch"),
    ('cur.executemany("DELETE FROM users WHERE id = %s", [1, 2])', None),
    ('cur.executemany("DELETE FROM users WHERE id = %s", [(1,), (2, 3)])', "param-count-mismatch"),
    (
        'cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2,)])',
        "param-count-mismatch",
    ),
    (
        'cur.executemany("DELETE FROM users WHERE id = %s", seq_of_args=[(1, 2)])',
        "param-count-mismatch",
    ),
]


@pytest.fixture(scope="module")
def conn(mysql_connect_args: dict[str, Any]) -> Iterator[rustmy.Connection]:
    with closing(rustmy.connect(**mysql_connect_args)) as conn:
        with conn.cursor() as cursor:
            cursor.execute(SCHEMA.replace("CREATE TABLE", "CREATE TEMPORARY TABLE"))
        yield conn


def _reported(source: str) -> list[str]:
    diagnostics = core.check_source(source, core.Settings(dialect="mysql", schema_sql=SCHEMA))

    return [d["rule"] for d in diagnostics if d["rule"].startswith("param-")]


def _run(source: str, conn: rustmy.Connection) -> None:
    namespace: dict[str, Any] = {}
    exec(compile(source, "<rustmy case>", "exec"), namespace)

    namespace["run"](conn)


@pytest.mark.parametrize("prepared", [False, True])
@pytest.mark.parametrize(("call", "rule"), CASES)
def test_reported_calls_fail_at_run_time(
    conn: rustmy.Connection, call: str, rule: str | None, prepared: bool
) -> None:
    source = SOURCE.format(call=call, prepared=prepared)

    reported = _reported(source)

    assert reported == ([] if rule is None else [rule])

    if rule is None:
        _run(source, conn)
    else:
        with pytest.raises(rustmy.Error):
            _run(source, conn)

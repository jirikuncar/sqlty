"""The check of a module against the other modules of a tree (``sqlty_python::index``)."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from sqlty.core import HelperIndex, ParsedSource, check_source

SCHEMA = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL, note TEXT);"
HEAD = """\
import sqlty
from typing import Annotated, Any, Literal, NamedTuple, NewType, TypeAlias, TypedDict, overload
from sqlty import SQL
"""
RULES = {"dynamic-query": "warn", "nullable-mismatch": "ignore"}


def check(modules: dict[str, str], app: str) -> int:
    """How many diagnostics ``app`` gets with ``modules`` indexed."""
    index = HelperIndex()
    for name, source in modules.items():
        index.add(name, ParsedSource(HEAD + source, name))
    found = check_source(
        ParsedSource(HEAD + app, "app"),
        schema_sql=SCHEMA,
        rules=RULES,
        helpers=index,
        module="app",
    )
    return len(found)


# Each was a difference between the Python passes and their port, found in review.
REVIEWED = {
    "imported params": (
        {"a": "class P(TypedDict):\n    x: int\n"},
        "from a import P\nclass Local(TypedDict):\n    v: int\n"
        "q = SQL[Local, P]('--sql\\nSELECT %(x)s::int AS v')\n",
        0,
    ),
    "imported generic": (
        {"a": "class Row[T](TypedDict):\n    v: T\n"},
        "from a import Row\nclass Other[T]: ...\n"
        "q1 = SQL[Row]('--sql\\nSELECT 1 AS v')\nq2 = SQL[Row[str]]('--sql\\nSELECT 1 AS v')\n",
        0,
    ),
    "trailing indent": (
        {},
        'def run(q: "Annotated[str, SQL]\\n "): ...\nrun("SELECT nope FROM t")\n',
        0,
    ),
    "quoted any": (
        {},
        "def run[T](q: Annotated[str, SQL], t: type[T]) -> T: ...\n"
        "run(x + y, \"'Any'\")\nrun(x + y, 'Any\\n ')\nrun(x + y, 'Any')\n",
        2,
    ),
    "class name": (
        {},
        "class Db\u093e:\n    def run(self, q: Annotated[str, SQL]): ...\n"
        "def f(db: 'Db\u093e', ok: 'Db\u093e '):\n    db.run('SELECT nope FROM t')\n",
        0,
    ),
    "from on its own line": (
        {"a": "def run(q: Annotated[str, SQL]): ...\n"},
        "from a import run\ndef f():\n    x = (yield\n        from run('SELECT nope FROM t'))\n",
        1,
    ),
    "call after an import": (
        {"a": "def run(q: Annotated[str, SQL]): ...\n"},
        "from a import run\nimport a; run('SELECT nope FROM t')\n",
        1,
    ),
    "cyclic rows": (
        {
            "a": "from b import RB\nclass RA(RB):\n    x: int\nclass RA2(TypedDict):\n    z: int\n",
            "b": "from a import RA2\nclass RB(TypedDict):\n    y: int\nclass RB2(RA2):\n    w: int\n",
        },
        "from a import RA\nfrom b import RB2\n"
        "SQL[RA]('--sql\\nSELECT 1 AS x')\nSQL[RB2]('--sql\\nSELECT 1 AS w')\n",
        1,
    ),
}


@pytest.mark.parametrize("case", REVIEWED.values(), ids=REVIEWED.keys())
def test_reviewed(case: tuple[dict[str, str], str, int]) -> None:
    modules, app, expected = case
    assert check(modules, app) == expected


def test_registered_file_that_does_not_parse(tmp_path: Path) -> None:
    helper = "def run(sql, row=None, params=()): ...\n"
    for name, text in (("broken", "def run(:\n"), ("big", helper + "#" * 300_000)):
        first, second = tmp_path / name / "one", tmp_path / name / "two"
        first.mkdir(parents=True)
        second.mkdir()
        (first / "ext.py").write_text(text)
        (second / "ext.py").write_text(helper)
        index = HelperIndex()
        index.register("ext.run", "sql", "row", roots=[first, second])
        # The first root holding the file ends the search: the helper takes keywords only.
        helper_read = index.modules["ext"]["run"]
        assert (helper_read.positional, helper_read.keywords) == ((), {"row", "sql"})


def test_a_fork_shares_what_its_parent_read() -> None:
    index = HelperIndex()
    assert index.add("db", HEAD + "def run(q: Annotated[str, SQL]): ...\n")
    assert not index.add("other", "x = 1\n")
    fork = index.fork()
    fork.register("lib.execute", "sql")
    assert sorted(fork.modules) == ["db", "lib"]
    assert sorted(index.modules) == ["db"]
    index.add("alias", "from db import run as zap\n")
    app = "from alias import zap\nzap('SELECT nope FROM t')\n"
    assert [
        d["rule"] for d in check_source(app, schema_sql=SCHEMA, helpers=index, module="app")
    ] == ["unknown-column"]
    # The fork was taken before `alias` was added.
    assert check_source(app, schema_sql=SCHEMA, helpers=fork, module="app") == []


def test_one_index_on_two_threads() -> None:
    """Modules importing each other's row types are read whole by the thread that is first."""
    modules, _, _ = REVIEWED["cyclic rows"]
    pad = "".join(f"class Pad{i}(TypedDict):\n    v{i}: int\n" for i in range(200))
    apps = [
        HEAD + "from a import RA\nSQL[RA]('--sql\\nSELECT 1 AS x')\n",
        HEAD + "from b import RB2\nSQL[RB2]('--sql\\nSELECT 1 AS w')\n",
    ]

    def count(index: HelperIndex, app: str) -> int:
        found = check_source(
            ParsedSource(app, "app"), schema_sql=SCHEMA, rules=RULES, helpers=index, module="app"
        )
        return len(found)

    for _ in range(20):
        index = HelperIndex()
        for name, source in modules.items():
            index.add(name, ParsedSource(HEAD + source + pad, name))
        index.rust()
        with ThreadPoolExecutor(2) as pool:
            raced = list(pool.map(lambda app, index=index: count(index, app), apps))
        assert sum(raced) == 1
        assert [count(index, app) for app in apps] == raced

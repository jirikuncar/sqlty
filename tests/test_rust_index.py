"""The check of a module against the other modules of a tree, in Rust and in Python.

Generated packages define helpers, row types and aliases, re-export them, import each
other in cycles and register configured functions; a module using them must get the same
diagnostics from ``sqlty_python`` with its ``index`` as from ``sqlty._check`` with a
``HelperIndex``. Raise the number of generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
from hypothesis import given
from hypothesis import strategies as st

from sqlty import _check
from sqlty._helpers import HelperIndex
from sqlty._source import ParsedSource
from tests.test_rust_check import QUERIES, RULES, SCHEMAS, arguments
from tests.test_rust_types import FUZZ, parses

DB = [
    "def run[T](q: Annotated[str, SQL], t: type[T], params=None) -> list[T]: ...\n",
    "def run(q: Annotated[str, SQL], *args): ...\n",
    "def fetch(sql: Annotated[str, sqlty.SQL], t=None, **params): ...\n",
    "def plain(query, row=None, params=()): ...\n",
    "@overload\ndef plain(query) -> None: ...\n@overload\ndef plain(query, row) -> int: ...\n",
    "class Db:\n"
    "    def one[T](self, q: Annotated[str, SQL], t: type[T], params=()) -> T: ...\n"
    "    @staticmethod\n    def st(q: Annotated[str, SQL], *args): ...\n"
    "    @classmethod\n    def cl[T](cls, q: Annotated[str, SQL], t: type[T], **params) -> T: ...\n"
    "    def raw(self, query, row=None, *params): ...\n",
    "from .types import Row, UserId\n",
    "from pkg import types\n",
]
TYPES = [
    "UserId = NewType('UserId', int)\n",
    "AdminId = NewType('AdminId', UserId)\n",
    "type Name = str\n",
    "Plain = str\nAlias: TypeAlias = Plain\n",
    "Status = Literal['new', 'old']\n",
    "class Row(TypedDict):\n    id: UserId\n    name: Name\n",
    "class Row(TypedDict):\n    id: int\n    name: 'Other'\n",
    "class Wide(Row):\n    note: Alias | None\n",
    "class Point(NamedTuple):\n    id: int\n    name: str\n",
    "from .more import Other, Loop\n",
    "from . import more\n",
    "Back = Loop\n",
    "class Gen[T](TypedDict):\n    id: T\n    name: str\n",
    "class P(TypedDict):\n    a: int\n",
]
MORE = [
    "from .types import UserId, Row, Back\n",
    "Other = str\n",
    "Other = UserId\n",
    "Loop = Back\n",
    "class Deep(Row):\n    score: float | None\n",
    "class Nested(TypedDict):\n    id: int\n    row: Row\n",
]
INIT = [
    "from .db import run, fetch, plain, Db\n",
    "from .db import run as go, Db as Database\n",
    "from .types import Row, UserId, Name, Status, Wide\n",
    "from .more import Deep as Row, Other\n",
    "from . import db, types\n",
    "from . import db as store\n",
    "import pkg.more\n",
]
IMPORTS = [
    "from pkg.db import run, fetch, plain, Db\n",
    "from pkg import run, Db, Row, UserId\n",
    "from pkg import go, Database as Db, Status, Wide\n",
    "from pkg import db, types, store\n",
    "import pkg\n",
    "import pkg.db, pkg.types as kinds\n",
    "from .db import run, Db\n",
    "from . import db, types as kinds\n",
    "from .. import pkg\n",
    "from .types import Row, UserId, Name, Status, Wide, Point, AdminId, Alias\n",
    "from pkg.types import Gen, P\n",
    "from pkg.more import Deep, Nested, Other, Loop\n",
    "from .missing import run, Row\n",
]
REGISTER = [
    ("pkg.db.plain", "query", "row", None),
    ("pkg.db.plain", "query", None, "other"),
    ("pkg.plain", "query", "row", None),
    ("pkg.db.Db.raw", "query", "row", None),
    ("pkg.Database.raw", "query", "row", "other"),
    ("pkg.db.missing", "query", "row", None),
    ("pkg.db.run", "q", "t", None),
    ("else.where.run", "sql", None, None),
    ("pkg.db.plain", "nope", "row", None),
]
ROWS = [
    "int",
    "Row",
    "Wide",
    "Point",
    "Deep",
    "Nested",
    "types.Row",
    "kinds.Wide",
    "pkg.Row",
    "pkg.types.Point",
    "Local",
    "Any",
    "Gen",
    "Gen[int]",
    "types.Gen[str]",
    "Local, P",
    "Gen[UserId], tuple[int]",
]
FIELDS = [
    "int",
    "str",
    "UserId",
    "AdminId",
    "Name",
    "Alias | None",
    "Status",
    "Other",
    "Loop",
    "types.UserId",
    "kinds.Name",
    "pkg.UserId",
    "pkg.types.Status",
    "pkg.more.Other",
    "Row",
]
HEAD = """\
import sqlty
from typing import Annotated, Any, Literal, NamedTuple, NewType, TypeAlias, TypedDict, overload
from sqlty import SQL
"""


@st.composite
def calls(draw: st.DrawFn) -> str:
    query = draw(st.sampled_from(QUERIES))
    quoted = draw(st.sampled_from([f'"{query}"', f'"""{query}"""', f'f"{query}"']))
    row = draw(st.sampled_from(ROWS))
    args = draw(arguments())
    named = draw(st.sampled_from(["", ", params=(1,)", ", params=[s, n]", ", a=1, b='x'"]))
    function = draw(
        st.sampled_from(
            [
                "run",
                "go",
                "fetch",
                "plain",
                "db.run",
                "store.fetch",
                "pkg.run",
                "pkg.db.plain",
                "pkg.db.run",
                "pkg.go",
                "Db.st",
                "Db.cl",
                "Db.one",
                "Db.raw",
                "Db().one",
                "repo.one",
                "repo.raw",
                "self.repo.raw",
                "db.Db.st",
                "pkg.Database.cl",
                "pkg.db.Db.raw",
                "local",
            ]
        )
    )
    line = draw(
        st.sampled_from(
            [
                f"{function}({quoted}, {row}{args})",
                f"{function}({quoted}{args})",
                f"{function}(q={quoted}, t={row}{named})",
                f"{function}(query={quoted}, row={row}{named})",
                f"SQL[{row}]({quoted})",
            ]
        )
    )
    return f"        x = {line}\n"


@st.composite
def trees(draw: st.DrawFn) -> tuple[dict[str, str], list[tuple[str, str, str | None, str | None]]]:
    def some(lines: list[str]) -> str:
        size = len(lines) // 2
        return HEAD + "".join(draw(st.lists(st.sampled_from(lines), min_size=size, unique=True)))

    fields = draw(st.lists(st.sampled_from(FIELDS), min_size=2, max_size=2))
    app = (
        some(IMPORTS)
        + draw(st.sampled_from(["", DB[0], "def local(q: Annotated[str, SQL], t=None): ...\n"]))
        + f"class Local(TypedDict):\n    id: {fields[0]}\n    name: {fields[1]}\n"
        + "repo: Db = make()\n"
        + "class Service:\n    repo: Db\n    def use(self, repo: Db, n: int, s: str):\n"
        + "".join(draw(st.lists(calls(), min_size=1, max_size=5)))
    )
    modules = {
        "pkg.__init__": some(INIT),
        "pkg.db": some(DB),
        "pkg.types": some(TYPES),
        "pkg.more": some(MORE),
        draw(st.sampled_from(["pkg.app", "app", "pkg.sub.app"])): app,
    }
    return modules, draw(st.lists(st.sampled_from(REGISTER), max_size=3, unique=True))


def compare(
    modules: dict[str, str],
    registered: list[tuple[str, str, str | None, str | None]],
    dialect: str = "postgres",
    rules: dict[str, str] | None = None,
    variants: int | None = None,
) -> int:
    """How many diagnostics both report for the last of ``modules``, -1 where Rust declines."""
    index = HelperIndex()
    for name, source in modules.items():
        index.add(name.removesuffix(".__init__"), ParsedSource(source, name))
    if registered:
        index = index.fork()
        for path, query, row, database in registered:
            index.register(path, query, row, database=database)
    name, source = list(modules.items())[-1]
    parsed = ParsedSource(source, name)
    usage = _check.RowUsage()
    settings = {
        "dialect": dialect,
        "schema_sql": SCHEMAS[dialect],
        "rules": rules,
        "import_style": None,
        "untyped": False,
        "module": name,
        "variants": variants,
    }
    found = _check._diagnostics(  # noqa: SLF001
        parsed, helpers=index, row_usage=usage, coerce=frozenset(), **settings
    )
    rust = index.rust()
    assert rust is not None
    declined = (
        _check._module_diagnostics(  # noqa: SLF001
            source, version=sys.version_info[:2], index=rust, name=name, **settings
        )
        is None
    )
    _check._compare_module(  # noqa: SLF001
        parsed, found, usage, coerce=frozenset(), index=rust, **settings
    )
    return -1 if declined else len(found)


APP = HEAD + "{imports}\nclass Service:\n    def use(self, repo: Db):\n        {call}\n"
CASES = {
    "imported": ("from pkg.db import run", "run('SELECT nope FROM t', int)"),
    "module": ("from pkg import db", "db.run('SELECT id, name FROM t', int)"),
    "re-exported": ("from pkg import run", "run('SELECT nope FROM t', int)"),
    "renamed": ("import pkg", "pkg.go('SELECT nope FROM t', int)"),
    "method": ("from pkg import Db", "repo.one('SELECT id FROM t', str)"),
    "registered": ("import pkg", "pkg.db.plain('SELECT nope FROM t')"),
    "row": ("from .types import Row", "SQL[Row]('--sql\\nSELECT name AS id, id AS name FROM t')"),
    "relative": ("from . import db", "db.Db.st('SELECT id FROM t WHERE id = %s')"),
}
TREE = {
    "pkg.__init__": HEAD + INIT[0] + INIT[1] + INIT[4],
    "pkg.db": HEAD + DB[0] + DB[3] + DB[5],
    "pkg.types": HEAD + TYPES[0] + TYPES[2] + TYPES[5],
    "pkg.more": HEAD,
}


@pytest.mark.parametrize("case", CASES.values(), ids=CASES.keys())
def test_cases(case: tuple[str, str]) -> None:
    app = APP.format(imports=case[0], call=case[1])
    assert compare({**TREE, "pkg.app": app}, [REGISTER[0]]) >= 1


@FUZZ
@given(trees(), st.sampled_from(sorted(SCHEMAS)), st.sampled_from(RULES))
def test_generated_trees(
    tree: tuple[dict[str, str], list[tuple[str, str, str | None, str | None]]],
    dialect: str,
    rules: dict[str, str] | None,
) -> None:
    modules, registered = tree
    if all(map(parses, modules.values())):
        compare(modules, registered, dialect, rules)


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
    tree = {name: HEAD + source for name, source in modules.items()}
    rules = {"dynamic-query": "warn", "nullable-mismatch": "ignore"}
    assert compare({**tree, "app": HEAD + app}, [], rules=rules) == expected


def test_registered_file_rust_does_not_read(tmp_path: Path) -> None:
    helper = "def run(sql, row=None, params=()): ...\n"
    for name, text in (("broken", "def run(:\n"), ("big", helper + "#" * 300_000)):
        first, second = tmp_path / name / "one", tmp_path / name / "two"
        first.mkdir(parents=True)
        second.mkdir()
        (first / "ext.py").write_text(text)
        (second / "ext.py").write_text(helper)
        index = HelperIndex()
        index.register("ext.run", "sql", "row", roots=[first, second])
        assert index.rust() is None


def test_names_follow_a_module_added_later() -> None:
    index = HelperIndex()
    index.add("db", ParsedSource(HEAD + DB[1], "db"))
    assert "zap" not in index.names
    index.add("alias", ParsedSource("from db import run as zap\n", "alias"))
    assert "zap" in index.names


def test_one_index_on_two_threads() -> None:
    """Modules importing each other's row types are read whole by the thread that is first."""
    modules, _, _ = REVIEWED["cyclic rows"]
    pad = "".join(f"class Pad{i}(TypedDict):\n    v{i}: int\n" for i in range(200))
    apps = [
        HEAD + "from a import RA\nSQL[RA]('--sql\\nSELECT 1 AS x')\n",
        HEAD + "from b import RB2\nSQL[RB2]('--sql\\nSELECT 1 AS w')\n",
    ]
    settings = {"version": sys.version_info[:2], "schema_sql": SCHEMAS["postgres"]}

    def check(index: object, app: str) -> int:
        found = _check._module_diagnostics(app, index=index, name="app", module="app", **settings)  # noqa: SLF001
        assert found is not None
        return len(found[0])

    for _ in range(20):
        index = HelperIndex()
        for name, source in modules.items():
            index.add(name, ParsedSource(HEAD + source + pad, name))
        rust = index.rust()
        with ThreadPoolExecutor(2) as pool:
            raced = list(pool.map(lambda app, rust=rust: check(rust, app), apps))
        assert sum(raced) == 1
        assert [check(rust, app) for app in apps] == raced

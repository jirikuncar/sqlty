"""Helper calls found from what Rust reads (``sqlty_python::helpers``) against ``ast``.

The corner cases pin down how a call is resolved where a name is bound more than once or
an import is unusual; both readings must agree on each, and the expected calls document
the behaviour. Generated modules then mix those imports and calls in every block kind.
Raise the number of generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import os

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty._helpers import HelperIndex, _comparable, _find_helper_calls, _site_calls
from sqlty._source import ParsedSource

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))
HEADER = "from typing import Annotated\nfrom sqlty import SQL\n"
DB = HEADER + (
    ""
    "def fetch_all[T](query: Annotated[str, SQL], form: type[T], /, **params: object) -> list[T]: ...\n"
    "def execute(query: Annotated[str, SQL], params: tuple[object, ...] = ()) -> None: ...\n"
    "class Db:\n"
    "    def fetch(self, query: Annotated[str, SQL], *args: object) -> None: ...\n"
    "    @staticmethod\n"
    "    def run(query: Annotated[str, SQL]) -> None: ...\n"
    "    @classmethod\n"
    "    def make(cls, query: Annotated[str, SQL]) -> None: ...\n"
)
OTHER = HEADER + "def fetch_all(query: Annotated[str, SQL], /) -> None: ...\n"
PACKAGE = "from .db import fetch_all as fa, Db\nfrom . import db\n"


@pytest.fixture(scope="module")
def index() -> HelperIndex:
    found = HelperIndex()
    assert found.add("pkg.db", DB)
    assert found.add("other", OTHER)
    found.add("pkg", ParsedSource(PACKAGE, "pkg.__init__"))
    return found


def both(text: str, index: HelperIndex, module: str = "app.use") -> list[tuple[int, str | None]]:
    parsed = ParsedSource(text)
    rust = _find_helper_calls(ParsedSource(text), index, module, variants=None, rust=True)
    python = _find_helper_calls(parsed, index, module, variants=None, rust=False)
    assert list(map(_comparable, rust)) == list(map(_comparable, python)), text
    return [(call.line, call.query) for call in python]


CASES = {
    # A local definition does not shadow an imported helper: resolution is by imports only.
    "local def keeps the import": (
        "from pkg.db import fetch_all\ndef fetch_all(q, t): ...\nfetch_all('SELECT 1', int)\n",
        [(3, "SELECT 1")],
    ),
    "assignment keeps the import": (
        "from pkg.db import fetch_all\nfetch_all = print\nfetch_all('SELECT 1', int)\n",
        [(3, "SELECT 1")],
    ),
    # Of two imports binding one name, the later in `ast.walk` order (breadth first) wins,
    # whichever branch runs.
    "later import wins": (
        "from pkg.db import fetch_all\nfrom other import fetch_all\nfetch_all('SELECT 1')\n",
        [(3, "SELECT 1")],
    ),
    "deeper import wins over an earlier shallow one": (
        "try:\n    from other import fetch_all\nexcept ImportError:\n    pass\n"
        "from pkg.db import fetch_all\nfetch_all('SELECT 1', int)\n",
        [(6, "SELECT 1")],
    ),
    "type checking import": (
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.db import execute\n"
        "execute('SELECT 1')\n",
        [(4, "SELECT 1")],
    ),
    # An import in a function binds the name for the whole file.
    "function level import is file wide": (
        "def f():\n    from pkg.db import execute\nexecute('SELECT 1')\n",
        [(3, "SELECT 1")],
    ),
    "module import": (
        "import pkg.db\nimport pkg.db as d\nfrom pkg import db\n"
        "pkg.db.execute('SELECT 1')\nd.execute('SELECT 2')\ndb.execute('SELECT 3')\n",
        [(4, "SELECT 1"), (5, "SELECT 2"), (6, "SELECT 3")],
    ),
    # Only a file naming a helper as defined is looked at (`HelperIndex.mentions`), so one
    # calling it only by the name a package re-exports it as is not.
    "package re-export under another name": (
        "from pkg import fa\nimport pkg\nfa('SELECT 1', int)\npkg.fa('SELECT 2', int)\n",
        [],
    ),
    "package re-export, file also naming the helper": (
        "from pkg import fa\nimport pkg\nfa('SELECT 1', int)\npkg.fa('SELECT 2', int)\n"
        "# fetch_all\n",
        [(3, "SELECT 1"), (4, "SELECT 2")],
    ),
    "star import is not followed": ("from pkg.db import *\nexecute('SELECT 1')\n", []),
    "dynamic import is not followed": (
        "import importlib\nimportlib.import_module('pkg.db').execute('SELECT 1')\n"
        "__import__('pkg.db').db.execute('SELECT 2')\n",
        [],
    ),
    "relative import": ("from .db import execute\nexecute('SELECT 1')\n", []),
    # `sqlty.drivers.psycopg.execute` is not the helper `execute` of the index.
    "driver helper of the same name": (
        "from sqlty.drivers.psycopg import execute\nexecute(conn, 'SELECT 1')\n",
        [],
    ),
    "methods": (
        "from pkg.db import Db\n"
        "Db.run('SELECT 1')\nDb.make('SELECT 2')\nDb.fetch('SELECT 3')\n"
        "db: Db = Db()\ndb.fetch('SELECT 4', 1)\n"
        "class Repo:\n"
        "    def __init__(self):\n        self.db = Db()\n"
        "    def go(self):\n        self.db.fetch('SELECT 5')\n"
        "    @staticmethod\n    def stat(self):\n        self.db.fetch('SELECT 6')\n",
        [(2, "SELECT 1"), (3, "SELECT 2"), (6, "SELECT 4"), (11, "SELECT 5")],
    ),
    # A class body never mentioning a helper name is skipped, as `ParsedSource.calls_to` does.
    "aliased call in a scope that never names the helper": (
        "from pkg.db import execute as run\ndef f():\n    run('SELECT 1')\nrun('SELECT 2')\n",
        [(3, "SELECT 1"), (4, "SELECT 2")],
    ),
    "keywords and starred arguments": (
        "from pkg.db import execute, fetch_all\n"
        "execute(query='SELECT 1', params=(1, 'a'))\nexecute(*q)\n"
        "fetch_all('SELECT 2', int, a=1, **k)\nfetch_all('SELECT 3', int, a=1.5)\n",
        [(2, "SELECT 1"), (4, "SELECT 2"), (5, "SELECT 3")],
    ),
    "implicit concatenation and multiline rows": (
        "from pkg.db import fetch_all\nrows = fetch_all(\n    'SELECT 1'\n    ' AS a',\n"
        "    tuple[\n        int\n    ],\n)\n",
        [(2, "SELECT 1 AS a")],
    ),
    "unicode before the row": (
        "from pkg.db import fetch_all\nx = 'žluť'; rows = fetch_all('SELECT 1', tuple[int])\n",
        [(2, "SELECT 1")],
    ),
    # CPython and ruff both normalize identifiers (NFKC), but `HelperIndex.mentions` reads the
    # raw text, so a call spelled with compatibility characters is never looked for.
    "compatibility characters": (
        "from pkg.db import ｅｘｅｃｕｔｅ\nｅｘｅｃｕｔｅ('SELECT 1')\n",
        [],
    ),
}


@pytest.mark.parametrize(("text", "expected"), CASES.values(), ids=CASES.keys())
def test_corner_cases(text: str, expected: list[tuple[int, str]], index: HelperIndex) -> None:
    assert both(text, index) == expected
    assert both(text.replace("\n", "\r\n"), index) == expected


@pytest.mark.parametrize(
    "text",
    [
        "from pkg.db import execute\nq = 'SELECT 1'\nexecute(q)\n",
        "from pkg.db import execute\nexecute(f'SELECT {1}')\n",
        "from pkg.db import execute\nexecute('SELECT ' + x)\n",
        "﻿from pkg.db import execute\nexecute('SELECT 1')\n",
        "from pkg.db import execute\nexecute('SELECT 1')\nx = " + "(" * 201 + ")" * 201 + "\n",
    ],
    ids=["name", "f-string", "concatenation", "byte order mark", "too deep"],
)
def test_undecided_files_fall_back_to_ast(text: str, index: HelperIndex) -> None:
    assert _site_calls(ParsedSource(text), index, "app.use") is None
    both(text, index)


def test_files_defining_helpers_use_ast(index: HelperIndex) -> None:
    text = DB + "fetch_all('SELECT 1', int)\n"
    assert both(text, index, "pkg.db") == [(len(text.splitlines()), "SELECT 1")]


IMPORTS = [
    "from pkg.db import fetch_all",
    "from pkg.db import fetch_all as f",
    "from pkg.db import execute",
    "from pkg.db import execute as f",
    "from pkg.db import Db",
    "from pkg.db import Db as D",
    "from other import fetch_all",
    "from pkg import fa",
    "from pkg import fa as f",
    "from pkg import db",
    "import pkg.db",
    "import pkg.db as db",
    "import pkg",
    "from sqlty.drivers.psycopg import execute",
    "from . import db",
]
CALLEES = [
    "fetch_all",
    "f",
    "execute",
    "fa",
    "Db.run",
    "Db.make",
    "Db.fetch",
    "D.run",
    "db.execute",
    "db.fetch_all",
    "pkg.db.execute",
    "pkg.fa",
    "pkg.Db.run",
    "self.fetch",
    "self.db.fetch",
    "r.fetch",
]
ARGUMENTS = [
    "'SELECT 1'",
    "'SELECT 1', int",
    "'SELECT 1', tuple[int, str]",
    "'SELECT 1', (1, 'a')",
    "query='SELECT 1'",
    "'SELECT 1', form=int",
    "'SELECT 1', int, a=1, b=None",
    "'SELECT 1', 1, d",
    "'SELECT 1', *a",
    "*a",
    "'SELECT %s', params=[1.5]",
    "'SELECT 1', int, **k",
    '"""SELECT\n    1""", int',
    "q, int",
    "f'SELECT {n}'",
]
BINDINGS = [
    "r = Db()",
    "r: Db = make()",
    "self.db = Db()",
    "d = date(2020, 1, 1)",
    "fetch_all = None",
    "def execute(): pass",
]
calls = st.builds(lambda c, a: f"{c}({a})", st.sampled_from(CALLEES), st.sampled_from(ARGUMENTS))
statements = st.one_of(
    st.sampled_from(IMPORTS),
    st.sampled_from(BINDINGS),
    calls,
    calls.map(lambda call: f"x = [{call} for _ in y]"),
)


def _wrap(body: list[str], kind: str) -> list[str]:
    inner = [f"    {line}" for line in body] or ["    pass"]
    match kind:
        case "if":
            return ["if c:", *inner, "else:", *inner]
        case "try":
            return ["try:", *inner, "except ImportError:", *inner]
        case "def":
            return ["def f(r: Db, n: int = 1):", *inner]
        case "class":
            return [
                "class C:",
                "    def __init__(self):",
                "        self.db = Db()",
                "    @staticmethod" if "static" in body[0] else "    @deco",
                "    def m(self):",
                *[f"    {line}" for line in inner],
            ]
    return body


blocks = st.recursive(
    st.lists(statements, min_size=1, max_size=5),
    lambda inner: st.builds(
        lambda body, kind, rest: _wrap(body, kind) + rest,
        inner,
        st.sampled_from(["if", "try", "def", "class"]),
        inner,
    ),
    max_leaves=10,
)


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=list(HealthCheck))
@given(
    st.lists(st.sampled_from(IMPORTS), min_size=1, max_size=4),
    blocks,
    st.sampled_from(["\n", "\r\n"]),
)
def test_generated_modules(index: HelperIndex, imports: list[str], lines: list[str], newline: str):
    text = newline.join(["from datetime import date", *imports, *lines]) + newline
    try:
        ast.parse(text)
    except SyntaxError:
        return
    both(text, index)

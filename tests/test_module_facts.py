"""Differential test of ``sqlty_python::module``: what Rust reads of a module against ``ast``.

Generated modules nest imports, aliases, driver bindings and calls in every block kind; the
corpus is every Python file of the repository and every Python block of the Markdown tests.
Raise the number of generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import os
import re
from pathlib import Path

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

import sqlty._source
from sqlty._drivers import _find_driver_calls
from sqlty._source import Facts, ParsedSource

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))
ROOT = Path(__file__).parent.parent
WRAPPERS = ("SQL", "sqlty.SQL", "Q", "x.SQL")


def python_facts(text: str) -> Facts:
    parsed = ParsedSource(text)
    assert parsed.tree is not None
    calls = [node for node in ast.walk(parsed.tree) if isinstance(node, ast.Call)]
    return Facts(
        sorted({c.func.id for c in calls if isinstance(c.func, ast.Name)}),
        sorted({c.func.attr for c in calls if isinstance(c.func, ast.Attribute)}),
        [
            (alias.asname or alias.name, alias.name, None, 0)
            if isinstance(node, ast.Import)
            else (alias.asname or alias.name, alias.name, node.module or "", node.level)
            for node in parsed.imports
            for alias in node.names
        ],
        [],
        sorted(parsed._typing_aliases().items()),
        [
            (
                call.line,
                call.query,
                None if call.params is None else [tuple(param) for param in call.params],
                call.driver,
                call.single,
                call.query_span,
            )
            for call in _find_driver_calls(parsed)
        ],
    )


def assert_same(text: str) -> None:
    try:
        ast.parse(text)
    except SyntaxError, ValueError:
        return
    rust = ParsedSource(text).facts
    assert rust is not None, f"ruff rejects what ast parses:\n{text}"
    python = python_facts(text)
    assert rust.names == python.names, text
    assert rust.attributes == python.attributes, text
    assert rust.imports == python.imports, text
    assert sorted(dict(rust.typing_aliases).items()) == python.typing_aliases, text
    assert rust.driver_calls == python.driver_calls, text
    any_calls = ParsedSource(text)
    with pytest.MonkeyPatch.context() as monkeypatch:
        monkeypatch.setattr(sqlty._source, "AST", "rust")
        assert ParsedSource(text).imported_names == any_calls._imported_names(), text
    for wrapper in WRAPPERS:
        found = [o for o, name in rust.any_calls if name == wrapper or name.endswith(f".{wrapper}")]
        assert found == any_calls._any_calls(wrapper), (wrapper, text)


IMPORTS = [
    "from typing import Any",
    "from typing import Any as A",
    "from typing import List as A",
    "from typing_extensions import Annotated as Ann, Any",
    "from typing import Annotated",
    "import typing as t",
    "import typing_extensions as t",
    "import os as t",
    "from m import run as r, fetch",
    "from . import run",
    "from .m import run as A",
    "from .typing import Any",
    "import psycopg",
    "import sqlite3",
    "import asyncpg, aiosqlite",
    "from sqlty.drivers.psycopg import execute, scalar",
    "from sqlty.drivers import psycopg as h",
    "from sqlty import SQL",
    "from datetime import date",
    "import datetime as dt",
    "from uuid import UUID as U",
]
SLICES = [
    "Any",
    "A",
    "t.Any",
    "'Any'",
    '"A"',
    "' Any'",
    "'t.Any'",
    "'Any'  'x'",
    "List",
    "int",
    "Ann[Any, 1]",
    "Annotated[A, 'x']",
    "t.Annotated[t.Any, 1]",
    "Annotated[int, Any]",
    "Annotated[Any]",
    "'Annotated[Any, 1]'",
    "tuple[Any]",
    "Any | None",
    "'('",
]
CALLEES = ["SQL", "sqlty.SQL", "Q", "x.SQL", "f().SQL", "a[0].SQL", "x.y.SQL", "(SQL)"]
BINDINGS = [
    "conn = psycopg.connect('')",
    "conn = await psycopg.AsyncConnection.connect('')",
    "c = sqlite3.connect('db')",
    "c = aiosqlite.connect('db')",
    "cur = conn.cursor()",
    "cur = c.cursor()",
    "pool = asyncpg.create_pool()",
    "x: sqlite3.Cursor",
    "x: 'psycopg.Cursor' = f()",
    "q = 'SELECT ?, ?'",
    "q = '--sql\\nSELECT %(a)s'",
    "q = SQL[int]('SELECT $1')",
    "q = sqlty.SQL[int]('SELECT 1', x)",
    "q = 'a' 'b'",
    "q, r = 'SELECT 1', 2",
    "d: date = date(2020, 1, 1)",
    "d = dt.datetime(1, 2, 3)",
    "d: 'U'",
    "n = 1",
    "n = int(s)",
    "n: list = []",
    "for c in y: pass",
    "c += 1",
    "(conn := psycopg.connect())",
    "w = [cur for cur in y]",
    "with sqlite3.connect('') as w, psycopg.connect() as (a, b): pass",
    "try: pass\nexcept E as conn: pass",
    "import sqlite3 as conn",
    "def cur(): pass",
    "class c: q = 'x'",
]
RECEIVERS = ["conn", "c", "cur", "pool", "w", "x", "a", "q"]
METHODS = ["execute", "executemany", "fetch", "fetchmany", "fetchval", "execute_fetchall", "cursor"]
QUERIES = [
    "'SELECT ?'",
    "q",
    "SQL[int]('SELECT $1, $2')",
    "sqlty.SQL[T]('x')",
    "f'x'",
    "b'x'",
    "('a'  'b')",
    "r",
    "*a",
]
ARGUMENTS = [
    "",
    ", (1, 'a')",
    ", [n, d, None]",
    ", {'a': -1, 'b': 1.5, 'c': True}",
    ", {'a': 1, **k}",
    ", {1: 2}",
    ", n",
    ", 'ab'",
    ", d",
    ", b'x'",
    ", [(1,), (2, 'a')]",
    ", ((n,), [*a], m)",
    ", params=(1,)",
    ", parameters=[1]",
    ", *a",
    ", **k",
    ", args=[(1,)]",
    ", dt.date(1, 2, 3)",
    ", (*a,)",
    ", (-True, +2, -1.5, 1j, -1j, --1)",
    ", {1, 2}, frozenset()",
    ", [x for x in y]",
    ", [None, None]",
    ", [[1], [2]]",
    ", f'{n}'",
    ", ...",
    ", U(x), date.today(), dt.time()",
    ", 1, 'a', d",
]
HELPERS = ["execute", "scalar", "h.execute", "h.scalar", "psycopg.execute"]
HELPER_ARGUMENTS = [
    "",
    ", (1,)",
    ", int",
    ", n",
    ", int, (1,)",
    ", params={'a': 1}",
    ", int, params=n",
    ", **k",
    ", [1], [2]",
]
driver_calls = st.one_of(
    st.builds(
        lambda r, m, q, a, aw: f"{aw}{r}.{m}({q}{a})",
        st.sampled_from(RECEIVERS),
        st.sampled_from(METHODS),
        st.sampled_from(QUERIES),
        st.sampled_from(ARGUMENTS),
        st.sampled_from(["", "await "]),
    ),
    st.builds(
        lambda h, q, a: f"{h}(conn, {q}{a})",
        st.sampled_from(HELPERS),
        st.sampled_from(QUERIES),
        st.sampled_from(HELPER_ARGUMENTS),
    ),
)
expressions = st.one_of(
    st.builds(
        lambda w, s: f"{w}[{s}]('--sql\\nSELECT 1')",
        st.sampled_from(CALLEES),
        st.sampled_from(SLICES),
    ),
    st.sampled_from(
        ["run(1)", "r(x)", "fetch()", "db.fetch_all(q)", "x.y.run()", "f()(1)", "(lambda: r())()"]
    ),
)


def _wrap(body: list[str], kind: str) -> list[str]:
    inner = [f"    {line}" for line in body] or ["    pass"]
    match kind:
        case "if":
            return ["if c:", *inner, "elif d:", *inner, "else:", *inner]
        case "try":
            return ["try:", *inner, "except E:", *inner, "finally:", *inner]
        case "def":
            return ["@deco(run(1))", "def f(a=r()):", *inner]
        case "async":
            return [
                "@deco(conn.execute('SELECT 1'))",
                "async def g(cur: psycopg.Cursor, /, n: int, *a: sqlite3.Connection, k: 'date' = d, **kw):",
                *inner,
            ]
        case "lambda":
            return ["v = lambda conn=c: conn.execute('SELECT ?', (1,))", *body]
        case "class":
            return ["class C:", *inner]
        case "match":
            return ["match x:", "    case 1:", *[f"    {line}" for line in inner]]
        case "with":
            return ["with r() as w:", *inner]
        case "for":
            return ["for i in y:", *inner, "else:", *inner]
    return body


statements = st.one_of(
    st.sampled_from(IMPORTS).map(lambda line: [line]),
    expressions.map(lambda e: [f"q = {e}"]),
    expressions.map(lambda e: [f"v = f'{{{e}}}'"]),
    expressions.map(lambda e: [f"w = [{e} for _ in y]"]),
    st.just(["s = 'žluťoučký kůň'  # ünïcode"]),
    st.sampled_from(BINDINGS).map(lambda line: line.split("\n")),
    driver_calls.map(lambda e: [e]),
    driver_calls.map(lambda e: [f"v = [{e} for _ in y]"]),
)
blocks = st.recursive(
    st.lists(statements, min_size=1, max_size=4).map(
        lambda parts: [line for part in parts for line in part]
    ),
    lambda inner: st.builds(
        lambda body, kind, rest: _wrap(body, kind) + rest,
        inner,
        st.sampled_from(["if", "try", "def", "async", "lambda", "class", "match", "with", "for"]),
        inner,
    ),
    max_leaves=12,
)


driver_statements = st.one_of(
    st.sampled_from(BINDINGS).map(lambda line: line.split("\n")),
    driver_calls.map(lambda e: [e]),
)
driver_blocks = st.recursive(
    st.lists(driver_statements, min_size=1, max_size=6).map(
        lambda parts: [line for part in parts for line in part]
    ),
    lambda inner: st.builds(
        lambda body, kind, rest: _wrap(body, kind) + rest,
        inner,
        st.sampled_from(["if", "try", "def", "async", "lambda", "class", "with", "for"]),
        inner,
    ),
    max_leaves=10,
)
driver_imports = st.lists(st.sampled_from(IMPORTS[-10:]), min_size=1, max_size=5, unique=True)


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=[HealthCheck.too_slow])
@given(blocks, st.sampled_from(["\n", "\r\n", "\r"]))
def test_generated_modules(lines: list[str], newline: str) -> None:
    assert_same("\n".join([*lines, ""]).replace("\n", newline))


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=[HealthCheck.too_slow])
@given(driver_imports, driver_blocks, st.sampled_from(["\n", "\r\n", "\r"]))
def test_generated_driver_calls(imports: list[str], lines: list[str], newline: str) -> None:
    assert_same("\n".join([*imports, *lines, ""]).replace("\n", newline))


def _corpus() -> list[tuple[str, str]]:
    found = [
        (str(path.relative_to(ROOT)), path.read_text())
        for path in sorted(ROOT.glob("**/*.py"))
        if ".venv" not in path.parts and "target" not in path.parts
    ]
    for path in sorted((ROOT / "tests" / "mdtest").glob("**/*.md")):
        for index, block in enumerate(
            re.findall(r"```python\n(.*?)```", path.read_text(), re.DOTALL)
        ):
            found.append((f"{path.relative_to(ROOT)}#{index}", block))
    return found


@pytest.mark.parametrize(
    ("name", "text"), _corpus(), ids=lambda value: value if "/" in value else ""
)
def test_corpus(name: str, text: str) -> None:
    assert_same(text)

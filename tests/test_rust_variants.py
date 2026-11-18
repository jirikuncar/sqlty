"""Query variants read in Rust (``sqlty_python::variants``).

Generated functions bind the parts of a query along ``if``, ``match``, loops and ``try``,
call functions of the module and compose with ``psycopg.sql``. Raise the number of
generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import functools
import sys

import pytest
from hypothesis import given
from hypothesis import strategies as st

from sqlty._core import call_variants
from tests.test_rust_check import RULES
from tests.test_rust_index import diagnosed
from tests.test_rust_types import FUZZ, parses

HEAD = """\
import psycopg
from psycopg import sql
from typing import Literal
from sqlty import SQL
import sqlty
TABLE = "users"
ORDER = {"new": "created DESC", "old": "created", TABLE: "id"}
COLS = sql.SQL(", ").join([sql.Identifier("a"), sql.Identifier("b")])
BASE = "SELECT id FROM " + TABLE
TWICE = "x"
TWICE = "y"
def where(kind: Literal["a", "b"], extra="") -> str:
    if kind == "a":
        return " WHERE a" + extra
    return " WHERE b"
def pick(flag, yes=" yes", *, no=" no"):
    return yes if flag else no
def loop(n):
    for _ in n:
        return "x"
    return "y"
def deep(k: Literal["p", "q"]):
    match k:
        case "p":
            return where("a", k)
        case _:
            return pick(k, no=k)
@decorated
def wrapped():
    return "w"
"""
NAMES = ["q", "x", "y", "kind", "mode", "flag", "TABLE", "TWICE", "BASE", "other"]
WORDS = ["' a'", "' b'", "'SELECT 1'", "''", "' {}'"]
TESTS = ["flag", "not flag", "kind == 'a'", "mode", "x", "flag and mode", "q == x", "(y := flag)"]


@functools.cache
def values() -> st.SearchStrategy[str]:
    name = st.sampled_from(NAMES)
    word = st.sampled_from(WORDS)
    atom = st.one_of(
        word,
        word,
        name,
        st.sampled_from(
            [
                "ORDER['new']",
                "ORDER[kind]",
                "ORDER[TABLE]",
                "ORDER[x]",
                "{'a': ' one', 'b': ' two'}[kind]",
                "{'a': x, 'a': y, **ORDER}['a']",
                "{'a': x, kind: y}['a']",
                "{1: x}[1]",
                "where(kind)",
                "where('a', x)",
                "where(kind=mode, extra=' AND c')",
                "where()",
                "where(*x)",
                "pick(flag)",
                "pick(flag, x, no=y)",
                "pick(0, no=' never')",
                "loop(x)",
                "deep(kind)",
                "deep('p')",
                "wrapped()",
                "unknown()",
                "SQL[int]('SELECT 2')",
                "SQL[int, tuple[int]]('SELECT %s')",
                "sqlty.SQL('SELECT 3')",
                "SQL[int](x)",
                "sql.SQL(' c')",
                "sql.Identifier(kind)",
                "sql.Identifier(TABLE, x)",
                "sql.SQL('SELECT {} FROM {}').format(COLS, sql.Identifier(kind))",
                "sql.SQL(' {} ').format(x)",
                "sql.SQL(x).format(sql.Literal(flag))",
                "sql.SQL(', ').join([x, y])",
                "sql.Placeholder(kind)",
                "COLS",
                "f' {x}'",
                "x.upper()",
                "x[1:]",
                "None",
                "1",
                "lambda: x",
                "[c for c in x]",
            ]
        ),
    )

    def extend(inner: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
        return st.one_of(
            st.builds("{} + {}".format, inner, inner),
            st.builds("({} if {} else {})".format, inner, st.sampled_from(TESTS), inner),
            st.builds("sql.SQL('{{}}{{}}').format({}, {})".format, inner, inner),
            st.builds("pick(flag, {}, no={})".format, inner, inner),
        )

    return st.recursive(atom, extend, max_leaves=4)


def indent(lines: list[str]) -> list[str]:
    return [f"    {line}" for line in lines] or ["    pass"]


@functools.cache
def statements() -> st.SearchStrategy[list[str]]:
    name = st.sampled_from(NAMES[:6])
    value = values()
    test = st.sampled_from(TESTS)
    simple = st.one_of(
        st.builds(lambda n, v: [f"{n} = {v}"], name, value),
        st.builds(lambda n, v: [f"{n} = {v}"], name, value),
        st.builds(lambda n, v: [f"{n} += {v}"], name, value),
        st.builds(lambda n, v: [f"{n}: str = {v}"], name, value),
        st.builds(lambda n, m, v, w: [f"{n}, {m} = {v}, {w}"], name, name, value, value),
        st.builds(lambda n, m, v: [f"{n} = {m} = {v}"], name, name, value),
        st.builds(lambda n, m, v: [f"[{n}, {m}] = {v}"], name, name, value),
        st.builds(lambda v: [f"run({v})"], value),
        st.builds(lambda v: [f"other.run({v})"], value),
        st.builds(lambda n: [f"{n}: str"], name),
        st.builds(lambda n: [f"del {n}"], name),
        st.builds(lambda n: [f"global {n}"], name),
        st.builds(lambda n: [f"import {n}"], name),
        st.builds(lambda n: [f"{n} -= 1"], name),
        st.builds(lambda n, v: [f"{n}.attr = {v}"], name, value),
        st.builds(lambda v: [f"return {v}"], value),
        st.sampled_from([["return"], ["raise ValueError"], ["break"], ["continue"], ["pass"]]),
    )

    def compound(block: st.SearchStrategy[list[str]]) -> st.SearchStrategy[list[str]]:
        case = st.sampled_from(
            ["'a'", "'a' | 'b'", "'b'", "_", "str() as x", "'a' if flag", "1", "[x, *y]", "z"]
        )
        return st.one_of(
            st.lists(block, min_size=1, max_size=3).map(lambda bs: [s for b in bs for s in b]),
            st.builds(lambda t, a: [f"if {t}:", *indent(a)], test, block),
            st.builds(
                lambda t, a, b: [f"if {t}:", *indent(a), "else:", *indent(b)], test, block, block
            ),
            st.builds(
                lambda t, u, a, b, c: [
                    f"if {t}:",
                    *indent(a),
                    f"elif {u}:",
                    *indent(b),
                    "else:",
                    *indent(c),
                ],
                test,
                test,
                block,
                block,
                block,
            ),
            st.builds(
                lambda t, u, a, b: [f"if {t}:", *indent(a), f"elif {u}:", *indent(b)],
                test,
                test,
                block,
                block,
            ),
            st.builds(
                lambda n, cases: [
                    f"match {n}:",
                    *[line for c, b in cases for line in indent([f"case {c}:", *indent(b)])],
                ],
                st.sampled_from(["kind", "mode", "x", "x.y", "(kind, mode)"]),
                st.lists(st.tuples(case, block), min_size=1, max_size=3),
            ),
            st.builds(lambda n, a: [f"for {n} in items:", *indent(a)], name, block),
            st.builds(
                lambda a, b: ["for _ in items:", *indent(a), "else:", *indent(b)], block, block
            ),
            st.builds(lambda t, a: [f"while {t}:", *indent(a)], test, block),
            st.builds(lambda n, a: [f"with ctx() as {n}:", *indent(a)], name, block),
            st.builds(
                lambda a, b, n, c: [
                    "try:",
                    *indent(a),
                    f"except E as {n}:",
                    *indent(b),
                    "finally:",
                    *indent(c),
                ],
                block,
                block,
                name,
                block,
            ),
            st.builds(lambda n, a: [f"def inner({n}=1):", *indent(a)], name, block),
            st.builds(lambda a: ["class Inner:", *indent(a)], block),
        )

    return st.recursive(simple, compound, max_leaves=10)


SIGNATURES = [
    "def build(flag: bool, kind: Literal['a', 'b'], mode: 'Literal[\"new\", \"old\"]', other=None):",
    "async def build(flag, kind: Literal['a'], /, mode: Literal['a', 1] = 'a', *x, **y):",
    "def build(kind: typing.Literal['a', 'b', 'a'], *, mode: Literal['new'], flag=lambda q: q):",
]


@st.composite
def modules(draw: st.DrawFn) -> str:
    body = draw(statements())
    outer = draw(st.booleans())
    lines = [draw(st.sampled_from(SIGNATURES)), *indent([*body, f"run({draw(values())})"])]
    if outer:
        lines = ["def outer(x, q='o'):", *indent(["y = ' outer'", *lines])]
    tail = draw(st.sampled_from(["", "", "x = ' m'\nrun(x + TWICE)\n", "def pick(): ...\n"]))
    return HEAD + "\n".join(lines) + "\n" + tail


def same(source: str, limit: int = 64) -> int:
    """How many calls Rust reads the variants of for ``source``."""
    found = call_variants(source, sys.version_info[:2], limit)
    assert found is not None
    return sum(bool(read) for _, read, *_ in found)


CASES = {
    "conditional": "    cond = ' a' if flag else ' b'\n    run(BASE + cond)\n",
    "branches": "    q = BASE\n    if flag:\n        q += ' x'\n    elif mode:\n        q += ' y'\n"
    "    run(q)\n",
    "literal": "    run(BASE + ORDER[mode] + where(kind))\n",
    "match": "    match kind:\n        case 'a':\n            q = ' one'\n        case _:\n"
    "            q = ' rest'\n    run(BASE + q)\n",
    "returns": "    if flag:\n        return\n    q = deep(kind)\n    run(q)\n",
    "declared": "    q = SQL[int, tuple[int]]('SELECT %s') if flag else SQL[int]('SELECT 1')\n"
    "    run(q)\n",
    "composed": "    q = sql.SQL('SELECT {} FROM {}').format(COLS, sql.Identifier(kind))\n    run(q)\n",
    "dynamic": "    q = sql.SQL('SELECT {}').format(sql.Identifier(other))\n    run(q)\n",
    "loop": "    q = BASE\n    for x in items:\n        q += ' x'\n    run(q)\n",
    "inside": "    q = BASE\n    try:\n        if flag:\n            q += ' f'\n        run(q)\n"
    "    finally:\n        pass\n",
    "composed unbound": "    if flag:\n        x = ' a'\n    run(sql.SQL(x))\n    run(BASE)\n",
    "starred": "    run(pick(*TABLE))\n    run(loop(TABLE))\n    run(BASE)\n",
    "unpassed": "    run(deep())\n    run(pick())\n    run(BASE)\n",
    "patterns": "    match kind:\n        case 'a' | 1:\n            q = ' one'\n        case _:\n"
    "            q = ' rest'\n    run(BASE + q)\n    run(BASE)\n",
    "annotations": "    run(odd(kind, mode))\n    run(BASE)\n"
    "def odd(k: 'Literal[', j: f().Literal['a']):\n    return k + j\n",
    "values": "    c = sql.Composed([None, True, 1, 1.5, b'x', ..., [1], sql.Identifier('t'),"
    " sql.Placeholder('p'), sql.SQL('s')])\n    d = {'k': c, 'j': 'x'}\n"
    "    if flag:\n        q = ' a'\n    else:\n        q = ' a'\n"
    "    run(sql.SQL('{}{}').format(d['k'], sql.SQL(q)))\n    run(BASE)\n",
    "dict": "    d = {'a': ' x', 'a': ' y'}\n    run(sql.SQL(', ').join(d))\n    run(d)\n"
    "    e = {'a': ' x' if flag else ' y'}\n    run(BASE + e['a'] + TABLE[0] + str(' x'))\n"
    "    q = SQL[int]('SELECT 1')\n    run(q.format())\n    run(SQL('SELECT 1'))\n"
    "    run(BASE + ANNOTATED + CHAINED + AUG + slots)\n"
    "ANNOTATED: str = ' a'\nCHAINED = OTHER = CHAINED = ' b'\nclass Klass: ...\n"
    "slots = {}\nslots['a'] = 1\nKlass.attr: int = 1\nAUG = ' a'\nAUG += ' b'\n"
    "import os.path, os\nfrom os import path, path\nslots.attr += 1\n",
    "statements": "    global G\n    d = {'k': ' x'}\n    d['k'].x.append(1)\n    f().append(1)\n"
    "    (await g()).append(1)\n    a, b = [' x', ' y']\n    [c, e] = (' x', ' y')\n"
    "    [h, i] = [' x', ' y']\n    j, d['j'] = ' x', ' y'\n    k, l = ' x', ' y', ' z'\n"
    "    m = n = ' m'\n    if (await flag):\n        q = ' a'\n    else:\n        q = ' b'\n"
    "    match kind:\n        case {'a': 1, **rest}:\n            q = ' r'\n"
    "        case [*others]:\n            q = ' o'\n        case _:\n            pass\n"
    "    def inner(z=lambda: 0):\n        nonlocal flag\n        flag = 1\n"
    "    run(BASE + a + c + h + j + q + m + n)\n    run(sql.SQL('{}'.format(b'x')))\n"
    "    run(BASE)\n",
    "flow": "    x: int\n    d = {'k': ' x'}\n    d.attr: str = ' a'\n    q = ' a'\n    q *= 2\n"
    "    d.x += ' a'\n    if flag:\n        q = ' a'\n    else:\n        raise ValueError\n"
    "    match other:\n        case 'a' | 'b':\n            q = ' b'\n    with lock:\n        pass\n"
    "    try:\n        pass\n    except E:\n        pass\n    except:\n        pass\n"
    "    await d.append(1)\n    print(1)\n    (a or b).append(1)\n"
    "    if flag(lambda: 0):\n        q = ' c'\n    run(BASE + q)\n    run(BASE + TABLE[0])\n"
    "    u = sql.SQL('{}'.format(b'x'))\n    run(u)\n    run(where(sql.SQL('{}'.format(b'x'))))\n"
    "    run(sql.Composed(['a\\x00']))\n    run(pick(**kw))\n    run(deep(other))\n"
    "    run(BASE + noret(flag) + twice() + wrapped() + lit('a', 'b'))\n"
    "    run(DEEP)\n    run(BASE)\n"
    "def noret(k):\n    if k:\n        return ' a'\n"
    "def twice():\n    return ' a'\ndef twice():\n    return ' b'\n"
    "def lit(k: Literal['a', 1], j: list['a'], z=lambda: 0):\n    return ' l'\n"
    "class Holder:\n    def method(self, flag):\n        run(BASE + (' a' if flag else ' b'))\n"
    "DEEP = " + "(" * 70 + "' a'" + " + ' b')" * 70 + "\n",
    "more": "    if flag:\n        q = ' a'\n    elif mode:\n        q = ' b'\n    else:\n        q = ' c'\n"
    "    flag = other\n    flag\n    run(BASE + q)\n    d = {'a': ' x'}\n    run(BASE + d['z'])\n"
    "    run(BASE + twice())\n    run(BASE + wrapped())\n    run(BASE + lit())\n"
    "    run(where(sql.SQL('{}'.format(b'x'))))\n    u = sql.SQL('{}'.format(b'x'))\n"
    "    match u:\n        case 'a':\n            run(BASE)\n    match u:\n        case 'a':\n"
    "            pass\n    run(BASE + q)\n    if flag:\n        z = sql.SQL('{}'.format(b'x'))\n"
    "        run(BASE + z)\n    def inner(z=lambda: 0, y=lambda w: w):\n"
    "        run(BASE + (' a' if flag else ' b'))\n    run(BASE)\n"
    "def twice():\n    return ' a'\ndef twice():\n    return ' b'\n"
    "def lit(k: Literal['a', 1], j: list['a']):\n    return k + j\n",
    "unread": "    match U:\n        case 'a':\n            run(BASE)\n    run(pick(U))\n"
    "    run(BASE + other())\n    run(BASE + lit2())\n    if flag:\n        z = U\n        run(BASE + z)\n"
    "    match other:\n        case 'a':\n            q = ' a'\n        case _:\n            q = ' b'\n"
    "    run(BASE + q)\n    match U:\n        case 'a':\n            pass\n    run(BASE)\n"
    "U = sql.SQL('{}'.format(b'x'))\ndef lit2(k: Literal['a', 1]):\n    return k\n"
    "def outer(z=lambda: 0):\n    def inner():\n        run(BASE + (' a' if flag else ' b'))\n",
    "nonlocal": "    nonlocal other\n    if flag:\n        other = ' a'\n    run(BASE + other)\n",
    "default": "    @run(BASE)\n    def inner(q=run(BASE)) -> run(BASE):\n        pass\n    run(BASE)\n",
    "loops": "    import os.path\n    class K: ...\n    while flag:\n        run(BASE + (' a' if mode else ' b'))\n"
    "    run(BASE + lit3(kind, other))\n"
    "    if mode:\n        r = ' a' if flag else ' a'\n        run(BASE + r + pick(mode))\n    run(BASE)\n"
    "def lit3(k: Literal['a', 'b'], j: Literal['x']):\n    return ' ' + k + j\n"
    "def outer(z=lambda w: w):\n    def inner():\n        run(BASE + (' a' if flag else ' b'))\n",
}


@pytest.mark.parametrize("body", CASES.values(), ids=CASES.keys())
def test_cases(body: str) -> None:
    assert same(HEAD + SIGNATURES[0] + "\n" + body) >= 1


def test_a_module_without_psycopg() -> None:
    assert (
        same("from sqlty import SQL\ndef build(flag):\n    run(other(flag))\n    run('SELECT 1')\n")
        >= 1
    )


def test_limits() -> None:
    flags = "".join(f"    q += ' a{i}' if f{i} else ' b{i}'\n" for i in range(13))
    source = HEAD + "def build(**f):\n    q = BASE\n" + flags + "    run(q)\n"
    assert same(source) >= 1
    assert same(HEAD + SIGNATURES[0] + "\n" + CASES["literal"], limit=3) >= 1


@FUZZ
@given(modules(), st.sampled_from([64, 64, 2, 1]))
def test_generated_variants(source: str, limit: int) -> None:
    if parses(source):
        same(source, limit)


PARTS = """\
import psycopg
from psycopg import sql
from typing import TypedDict
from sqlty import SQL
from .more import DEEP, P as Params
class P(TypedDict):
    a: int
    b: str | None
TABLE = "t"
BASE = "SELECT id, name FROM " + TABLE
BY_ID = SQL[int, tuple[int]]("SELECT id FROM t WHERE id = %s")
BY_NAME = SQL[int, P]("SELECT id FROM t WHERE id = %(a)s AND name = %(b)s")
BY_BOTH = SQL[int, Params]("SELECT id FROM t WHERE id = %(a)s")
BY_LIST = SQL[int, tuple[list[int], bool | None]]("SELECT id FROM t WHERE id = ANY(%s) AND flag = %s")
COLS = sql.SQL(", ").join([sql.Identifier("id"), sql.Identifier("name")])
ORDER = {"new": " ORDER BY born DESC", "old": " ORDER BY born"}
TWICE = "a"
TWICE = "b"
"""
MORE = """\
from typing import TypedDict
from .parts import BASE
class P(TypedDict):
    a: str
DEEP = BASE + " WHERE flag"
"""
DB = """\
from typing import Annotated
from sqlty import SQL
def run[T](q: Annotated[str, SQL], t: type[T] = int, params=None) -> list[T]: ...
def plain(query, row=None, *params): ...
"""
APP_IMPORTS = [
    "from pkg.parts import *\n",
    "from pkg.parts import BASE, BY_ID, BY_NAME, BY_BOTH, BY_LIST, COLS, ORDER, DEEP, TWICE, P\n",
    "from pkg import parts\nfrom pkg.parts import BASE as B, BY_ID as ONE\n",
    "from .parts import BASE, BY_NAME, COLS\nBY_ID = 'SELECT 1'\n",
    "import pkg.parts\nimport pkg.parts as p\n",
]
QUERY = [
    "BASE",
    "B + ' WHERE id = %s'",
    "BY_ID",
    "ONE",
    "BY_NAME",
    "BY_BOTH",
    "BY_LIST",
    "DEEP",
    "TWICE",
    "parts.BY_ID",
    "parts.BY_NAME",
    "pkg.parts.BY_BOTH",
    "p.DEEP + ORDER[mode]",
    "parts.BASE + parts.ORDER[mode]",
    "BASE + (' WHERE id = %s' if flag else ' WHERE name = %s')",
    "BASE + (' WHERE id = %s' if flag else '')",
    "(BY_ID if flag else BY_NAME)",
    "(BY_ID if flag else BASE)",
    "sql.SQL('SELECT {} FROM t').format(COLS)",
    "sql.SQL('SELECT {} FROM {}').format(parts.COLS, sql.Identifier(mode))",
    "sql.SQL('SELECT {} FROM t WHERE id = %s').format(sql.Identifier(mode))",
    "sql.SQL('SELECT nope FROM {}').format(sql.Identifier(other))",
    "q",
    "q + ' LIMIT 1'",
    "'SELECT nope FROM t' + mode",
    "f'SELECT {mode}'",
    "other",
]
BEFORE = [
    "",
    "q = BASE\n",
    "q = BY_ID\n",
    "q = parts.BY_NAME if flag else BY_ID\n",
    "q = BASE\nif flag:\n    q += ' WHERE id = %s'\n",
    "q = BASE\nif flag:\n    q += ' WHERE nope = 1'\nelif mode == 'new':\n    q += ' WHERE name = %s'\n",
    "if flag:\n    q = BY_ID\nelse:\n    q = 'SELECT name FROM t WHERE id = %s'\n",
    "match mode:\n    case 'new':\n        q = BY_NAME\n    case _:\n        q = BY_BOTH\n",
    "q = sql.SQL('SELECT {} FROM t').format(sql.Identifier(mode))\n",
    "q = BASE\nfor _ in other:\n    q += ' x'\n",
]
PASSED = [
    "",
    ", (1,)",
    ", ('x',)",
    ", (None, 2)",
    ", [flag]",
    ", ([1], None)",
    ", (['x'], 1)",
    ", {'a': 1, 'b': None}",
    ", {'a': 'x'}",
    ", {'a': 1, 'b': 2, 'c': 3}",
    ", params=(other,)",
    ", (mode,)",
]


@st.composite
def trees(draw: st.DrawFn) -> dict[str, str]:
    def call() -> str:
        before = draw(st.sampled_from(BEFORE))
        function = draw(st.sampled_from(["run", "db.run", "plain", "pkg.db.plain", "local"]))
        row = draw(st.sampled_from(["", ", int", ", Row", ", Any"]))
        passed = draw(st.sampled_from(PASSED)) if row else ""
        line = f"{function}({draw(st.sampled_from(QUERY))}{row}{passed})\n"
        return "".join(f"    {text}\n" for text in (before + line).splitlines())

    app = (
        "import psycopg\nfrom psycopg import sql\nimport pkg\n"
        "from typing import Annotated, Any, Literal, TypedDict\nfrom sqlty import SQL\n"
        "from pkg import db\nfrom pkg.db import run, plain\n"
        + draw(st.sampled_from(APP_IMPORTS))
        + "def local(q: Annotated[str, SQL], t=None, params=()): ...\n"
        + "class Row(TypedDict):\n    id: int\n    name: str\n"
        + draw(
            st.sampled_from(
                [
                    "def use(flag: bool, mode: Literal['new', 'old'], other):\n",
                    "def use(flag, mode: Literal['new'], other=None):\n",
                    "async def use(flag, mode, other):\n",
                ]
            )
        )
        + "".join(call() for _ in range(draw(st.integers(1, 4))))
    )
    name = draw(st.sampled_from(["pkg.app", "app"]))
    return {"pkg.__init__": "", "pkg.more": MORE, "pkg.parts": PARTS, "pkg.db": DB, name: app}


REGISTERED = [("pkg.db.plain", "query", "row", None), ("pkg.db.plain", "query", None, "other")]


@FUZZ
@given(
    trees(),
    st.lists(st.sampled_from(REGISTERED), max_size=1),
    st.sampled_from([None, 64, 64, 2, 1]),
    st.sampled_from([*RULES, {"dynamic-query": "warn"}]),
)
def test_generated_checks(
    modules: dict[str, str],
    registered: list[tuple[str, str, str | None, str | None]],
    variants: int | None,
    rules: dict[str, str] | None,
) -> None:
    if parses(list(modules.values())[-1]):
        diagnosed(modules, registered, rules=rules, variants=variants)

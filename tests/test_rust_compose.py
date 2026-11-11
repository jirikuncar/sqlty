"""``psycopg.sql`` compositions rendered in Rust (``sqlty_python::compose``) and by psycopg.

``sqlty._psycopg`` builds a composition with the real ``psycopg.sql`` classes; the Rust
model must render the same SQL, or say it does not cover the expression. Raise the number
of generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import sys

import pytest
from hypothesis import given
from hypothesis import strategies as st

from sqlty._core import rendered_compositions
from sqlty._source import ParsedSource
from tests.test_rust_types import FUZZ, parses

pytest.importorskip("psycopg")

HEAD = """\
import psycopg
from psycopg import sql
from psycopg import sql as s, pq
from psycopg.sql import SQL, Identifier as Ident, Literal, Placeholder, Composed
TABLE = "users"
PARTS = [sql.Identifier("a"), sql.Identifier("b")]
COLS = sql.SQL(", ").join(PARTS)
TWICE = "x"
TWICE = "y"
names = ["a"]
names.append("b")
"""
ATOMS = [
    "'a'",
    "'it''s'",
    "'a\\\\b'",
    "''",
    "'{}'",
    "'{} {}'",
    "'{0} {1}'",
    "'{a} {b}'",
    "'{0} {}'",
    "'{} {0}'",
    "'{{}}'",
    "'{'",
    "'}'",
    "'{a!r}'",
    "'{a:>3}'",
    "'{:}'",
    "'{a.b}'",
    "'{a[0]}'",
    "'{ }'",
    "', '",
    "'é'",
    "'%s'",
    "'a)'",
    "'s'",
    "'b'",
    "1",
    "0",
    "1.5",
    "True",
    "None",
    "b'x'",
    "...",
    "2j",
    "12345678901234567890123",
    "TABLE",
    "PARTS",
    "COLS",
    "TWICE",
    "names",
    "local",
    "param",
    "unknown",
    "pq.Format",
    "f'{param}'",
    "-1",
]
CALLEES = [
    "sql.SQL",
    "SQL",
    "s.SQL",
    "psycopg.sql.SQL",
    "sql.Identifier",
    "Ident",
    "sql.Literal",
    "Literal",
    "sql.Placeholder",
    "Placeholder",
    "sql.Composed",
    "Composed",
    "sql.Other",
    "str",
]


def wild() -> st.SearchStrategy[str]:
    """Any expression over the atoms, most of which psycopg rejects."""

    def extend(inner: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
        args = st.lists(inner, max_size=3).map(", ".join)
        kwargs = st.lists(
            st.tuples(st.sampled_from(["a", "b", "obj", "seq", "name", "format"]), inner),
            max_size=2,
            unique_by=lambda pair: pair[0],
        ).map(lambda pairs: "".join(f", {k}={v}" for k, v in pairs))
        call = st.builds(
            lambda callee, args, kwargs: f"{callee}({(args + kwargs).removeprefix(', ')})",
            st.sampled_from(CALLEES),
            args,
            kwargs,
        )
        method = st.builds(
            lambda receiver, name, args, kwargs: (
                f"{receiver}.{name}({(args + kwargs).removeprefix(', ')})"
            ),
            inner,
            st.sampled_from(["format", "join", "strip"]),
            args,
            kwargs,
        )
        return st.one_of(
            call,
            method,
            st.builds("{} + {}".format, inner, inner),
            st.builds("[{}]".format, args),
            st.builds("({},)".format, inner),
            st.builds("[*{}]".format, inner),
            st.builds("{} * 2".format, inner),
        )

    return st.recursive(st.sampled_from(ATOMS), extend, max_leaves=6)


TEXTS = [a for a in ATOMS if a.startswith("'")] + ["TABLE", "'SELECT {} FROM {}'", "'a = {a}'"]
SQL_CLASSES = ["sql.SQL", "SQL", "s.SQL", "psycopg.sql.SQL"]


def expressions() -> st.SearchStrategy[str]:
    """Expressions that mostly build a composition, some with a part gone wrong."""
    text = st.recursive(
        st.sampled_from(TEXTS),
        lambda inner: st.one_of(
            st.builds("{} + {}".format, inner, inner),
            st.builds("{}.format({})".format, inner, st.lists(inner, max_size=2).map(", ".join)),
            st.builds("{}.format(a={}, b=1)".format, inner, inner),
            st.builds("{}.join([{}])".format, inner, st.lists(inner, max_size=3).map(", ".join)),
            st.builds("{}.join({})".format, inner, inner),
        ),
        max_leaves=4,
    )
    template = st.builds("{}({})".format, st.sampled_from(SQL_CLASSES), text)

    def extend(inner: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
        part = st.one_of(inner, inner, text, st.sampled_from(["1", "None", "True", "1.5", "[1]"]))
        parts = st.lists(part, max_size=3).map(", ".join)
        return st.one_of(
            st.builds("{}.format({})".format, template, parts),
            st.builds("{}.format({}, a={}, b={})".format, template, part, part, part),
            st.builds("{}.join([{}])".format, template, parts),
            st.builds("{}.join(({},))".format, template, part),
            st.builds("{}.join({})".format, template, inner),
            st.builds("{} + {}".format, inner, inner),
            st.builds("{} + {}".format, inner, part),
            st.builds("sql.Composed([{}])".format, parts),
            st.builds("Composed({})".format, part),
        )

    names = st.lists(text, min_size=1, max_size=2).map(", ".join)
    atom = st.one_of(
        template,
        template,
        st.builds("sql.Identifier({})".format, names),
        st.builds("Ident({})".format, text),
        st.builds("sql.Literal({})".format, wild()),
        st.sampled_from(["sql.Placeholder()", "Placeholder('n')", "sql.Placeholder('n', 't')"]),
        st.builds("sql.Placeholder(name={}, format={})".format, text, text),
        st.sampled_from(["COLS", "local", "param", "PARTS", "sql.SQL(', ').join(PARTS)"]),
    )
    return st.one_of(st.recursive(atom, extend, max_leaves=6), wild())


@st.composite
def modules(draw: st.DrawFn) -> str:
    lines = [f"    run({draw(expressions())})\n" for _ in range(draw(st.integers(1, 4)))]
    return (
        HEAD
        + f"def build(param, other={draw(expressions())}):\n"
        + f"    local = {draw(expressions())}\n"
        + "".join(lines)
        + f"run({draw(expressions())})\n"
        + draw(st.sampled_from(["", "local = 1\n", "def param(): ...\n", "TABLE = 'again'\n"]))
    )


def same(source: str) -> int:
    """How many compositions both render for ``source``; raises where they differ."""
    rust = rendered_compositions(source, sys.version_info[:2])
    assert rust is not None
    parsed = ParsedSource(source)
    python = sorted(
        (parsed.span(call.args[0])[0], parsed.psycopg.render(call.args[0]))
        for call in parsed.calls
        if isinstance(call.func, ast.Name) and call.args
    )
    assert [at for at, _, _ in rust] == [at for at, _ in python]
    covered = [(at, text) for at, read, text in rust if read]
    assert covered == [found for found, (_, read, _) in zip(python, rust, strict=True) if read]
    return len(covered)


CASES = {
    "format": "sql.SQL('SELECT {} FROM {t}').format(sql.Identifier('a', 'b'), t=Ident(TABLE))",
    "join": "sql.SQL(', ').join([sql.Identifier('a'), sql.Placeholder('b'), 1, 'x', None])",
    "add": "sql.SQL('SELECT ') + COLS + sql.SQL(' FROM ') + sql.Identifier(TABLE)",
    "literal": "sql.SQL('WHERE a = {}').format(sql.Literal(unknown))",
    "escapes": "sql.SQL('{{}} {}').format('it''s a\\\\b')",
    "composed": "sql.Composed(['a', sql.SQL('b'), True])",
    "placeholder": "SQL('{} {}').format(Placeholder(), Placeholder('n', 't'))",
    "name": "sql.SQL('SELECT 1 FROM ') + local",
    "text": "sql.SQL('{}'.format('a') + ', '.join(['b', 'c']))",
}


@pytest.mark.parametrize("expression", CASES.values(), ids=CASES.keys())
def test_cases(expression: str) -> None:
    source = HEAD + f"def build(param):\n    local = sql.SQL('t')\n    run({expression})\n"
    assert same(source) >= 1
    assert rendered_compositions(source, sys.version_info[:2])[0][2] not in (None, "unchecked")


@FUZZ
@given(modules())
def test_generated_compositions(source: str) -> None:
    if parses(source):
        same(source)

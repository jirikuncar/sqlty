"""Edges of queries built from parts, found by review: what CPython and psycopg do there."""

from __future__ import annotations

import time

import pytest

from sqlty._helpers import HelperIndex
from sqlty._source import AST, AstMismatch, ParsedSource
from sqlty.core import check_source

SCHEMA = "CREATE TABLE t (id INTEGER NOT NULL, note TEXT);"
RULES = {"dynamic-query": "warn", "unchecked-query": "warn"}
HEAD = """\
import psycopg
from psycopg import sql
from typing import Annotated, Literal
from sqlty import SQL

def run[T](q: Annotated[str, SQL], t: type[T] = int, params=None) -> list[T]: ...

"""


def found(body: str) -> list[tuple[int, str, str]]:
    diagnostics = check_source(HEAD + body, schema_sql=SCHEMA, rules=RULES, query_variants=64)
    line = HEAD.count("\n")
    return [(d["line"] - line, d["rule"], d["message"]) for d in diagnostics]


def test_format_without_fields_keeps_the_declared_params():
    body = """\
BY_ID = SQL[int, tuple[int]]("SELECT id FROM t WHERE id = %s")
def f():
    run(BY_ID.format(), int, ("x",))
"""
    assert found(body) == [(3, "param-type-mismatch", "parameter 1 expects int, got str")]


@pytest.mark.parametrize(("prefix", "variants"), [("", 2), ("U", 2), ("u", 4)])
def test_only_a_lowercase_u_prefix_tells_conditions_apart(prefix, variants):
    body = f"""\
def f(other):
    a = "SELECT id" if other == "a" else "SELECT 1"
    b = " FROM t" if other == {prefix}"a" else ""
    run(a + b, int)
"""
    assert len(found(body)) == (variants == 4)


def test_variadic_literal_is_reported_on_the_line_of_its_name():
    body = """\
def f(*
      args: Literal["id", "nope"]):
    run("SELECT " + args + " FROM t", int)
"""
    ((_, _, message),) = found(body)
    assert message.endswith(f"(variant: line {HEAD.count(chr(10)) + 2})")


def test_placeholder_format_must_be_a_plain_str():
    body = """\
def f():
    fmt = SQL[int]("s")
    run(sql.SQL("SELECT id FROM t WHERE id = {}").format(sql.Placeholder("x", fmt)), int)
"""
    assert [rule for _, rule, _ in found(body)] == ["unchecked-query"]


def test_infinite_float_is_formatted_as_inf():
    body = 'def f():\n    run("SELECT id FROM t WHERE id = {}".format(1e999), int)\n'
    assert found(body) == [(2, "unknown-column", "column `inf` does not exist in `t`")]


def test_field_named_with_another_digit_is_not_read_as_a_keyword():
    body = 'def f():\n    run("SELECT {١} FROM t".format("id", "nope"), int)\n'
    if AST == "compare":
        with pytest.raises(AstMismatch):
            found(body)
        return
    # Python reads the digit as an index, as CPython does; Rust leaves the query unchecked.
    expected = "unchecked-query" if AST == "rust" else "unknown-column"
    assert [rule for _, rule, _ in found(body)] == [expected]


def test_bare_generator_argument_spans_the_parentheses():
    source = HEAD + "def f(other):\n    run(q for q in other)\n"
    (diagnostic,) = check_source(source, schema_sql=SCHEMA, rules=RULES)
    assert source[diagnostic["start"] : diagnostic["end"]] == "(q for q in other)"


@pytest.mark.timing
def test_many_variants_are_counted_in_linear_time():
    names = [f"p{i}" for i in range(15)]
    params = ", ".join(f"{name}: Literal['a{name}', 'b{name}']" for name in names)
    started = time.perf_counter()
    (diagnostic,) = found(f"def build({params}):\n    run({' + '.join(names)})\n")
    assert "more than max-query-variants" in diagnostic[2]
    assert time.perf_counter() - started < 3


@pytest.mark.timing
def test_constants_of_a_large_module_are_read_once():
    parts = "".join(f"C{i} = 'SELECT id FROM t WHERE id = {i}'\n" for i in range(4000))
    calls = "".join(f"def f{i}():\n    run(parts.C{i}, int)\n" for i in range(400))
    app = "from db import run\nimport parts\n" + calls
    index = HelperIndex()
    for name, text in (("db", HEAD), ("parts", parts), ("app", app)):
        index.add(name, ParsedSource(text, name))
    started = time.perf_counter()
    assert check_source(ParsedSource(app, "app"), schema_sql=SCHEMA, helpers=index) == []
    assert time.perf_counter() - started < 3

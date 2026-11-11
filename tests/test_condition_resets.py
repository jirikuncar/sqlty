"""A condition tested twice takes the same branch only while what it reads cannot change."""

from __future__ import annotations

import ast

import pytest

from sqlty._source import ParsedSource
from sqlty._variants import Unchecked, query_variants

BOTH = ["x1", "y2"]
EVERY = ["x1", "x2", "y1", "y2"]


def variants(test: str, between: str, header: str = "def f(n, o, xs, t):") -> list[str]:
    """The variants of the query."""
    source = f"""\
{header}
    if {test}:
        a = 'x'
    else:
        a = 'y'
    {between}
    if {test}:
        b = '1'
    else:
        b = '2'
    run(a + b)
"""
    parsed = ParsedSource(source)
    call = next(c for c in parsed.calls if isinstance(c.func, ast.Name) and c.func.id == "run")
    found = query_variants(parsed, call, call.args[0], 64)
    assert not isinstance(found, Unchecked)
    return sorted(str(variant.query) for variant in found)


@pytest.mark.parametrize(
    ("test", "between"),
    [
        ("n", "pass"),
        ("n", "print(n)"),
        ("n", "m = n"),
        ("o.on", "other.on = 1"),
        ("check()", "pass"),
        ("o.on", "toggle(o)"),
        ("o.on", "x = o.toggle()"),
        ("(lambda: (n := 1))", "pass"),
        ("n", "[x for x in xs if (lambda: (n := 1))]"),
    ],
)
def test_condition_holds(test, between):
    assert variants(test, between) == BOTH


@pytest.mark.parametrize(
    ("test", "between"),
    [
        ("n", "n = 0"),
        ("n", "n -= 1"),
        ("n", "n, m = t"),
        ("n", "del n"),
        ("o.on", "o.on = not o.on"),
        ("o.on", "o.on += 1"),
        ("o.on", "del o.on"),
        ("o.on", "o.toggle()"),
        ("o.inner.on", "o.inner.reset()"),
        ("xs", "xs.append(1)"),
        ("xs", "xs[0] = 1"),
        ("xs", "del xs[0]"),
        ("xs", "xs[0].name = 1"),
        ("n", "[(n := x) for x in xs]"),
        ("(n := o())", "pass"),
        ("n", "for o.on, n in t: pass"),
        ("o.on", "with t as o.on: pass"),
        ("xs", "xs.items().clear()"),
        ("o.on", "if n: o.on = 1"),
        ("o.on", "o.inner[0].on = 1"),
        ("(lambda: (n := 1)) and (m := n)", "pass"),
    ],
)
def test_condition_is_reset(test, between):
    assert variants(test, between) == EVERY


@pytest.mark.parametrize("test", ["await o.get()", "await o"])
def test_awaited_condition_never_holds(test):
    assert variants(test, "pass", "async def f(n, o, xs, t):") == EVERY
    assert variants("o.on", "await o.refresh()", "async def f(n, o, xs, t):") == EVERY

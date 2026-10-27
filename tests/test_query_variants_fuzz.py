"""Differential test of query variants: generated functions, run for every input, against what sqlty reads.

Off by default; run it on demand with::

    SQLTY_FUZZ=1 uv run pytest tests/test_query_variants_fuzz.py
    SQLTY_FUZZ=1 SQLTY_FUZZ_EXAMPLES=5000 uv run pytest tests/test_query_variants_fuzz.py

Each example is a function of three booleans built from ``if``/``elif``/``else``,
``match``, conditional expressions, ``+=``, ``return`` and ``raise`` over string literals.
The generated code (never user code) runs for all eight inputs to collect the queries it
passes to ``run``. Every one of them must be a variant sqlty reads (soundness); when each
condition is a plain parameter tested the same way everywhere, the variants are exactly
those queries (precision).
"""

from __future__ import annotations

import ast
import contextlib
import itertools
import os
import textwrap

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty._source import ParsedSource
from sqlty._variants import Unchecked, query_variants

pytestmark = pytest.mark.skipif(not os.environ.get("SQLTY_FUZZ"), reason="set SQLTY_FUZZ=1 to fuzz")

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))

FLAGS = ("a", "b", "c")
words = st.sampled_from(["x", "y", "z", "w"])
plain = st.sampled_from(FLAGS)
negated = plain.map(lambda flag: f"not {flag}")


def _indent(lines: list[str]) -> list[str]:
    return [f"    {line}" for line in lines] or ["    pass"]


def _statements(conditions: st.SearchStrategy[str]) -> st.SearchStrategy[list[str]]:
    simple = st.one_of(
        words.map(lambda w: [f'q += " {w}"']),
        words.map(lambda w: [f'q = "{w}"']),
        words.map(lambda w: [f'x = "{w}"']),
        st.just(["q += x"]),
        st.tuples(conditions, words, words).map(
            lambda t: [f'q += " {t[1]}" if {t[0]} else " {t[2]}"']
        ),
        st.just(["return"]),
        st.just(["raise ValueError"]),
    )

    def compound(block: st.SearchStrategy[list[str]]) -> st.SearchStrategy[list[str]]:
        if_else = st.tuples(conditions, block, block).map(
            lambda t: [f"if {t[0]}:", *_indent(t[1]), "else:", *_indent(t[2])]
        )
        if_only = st.tuples(conditions, block).map(lambda t: [f"if {t[0]}:", *_indent(t[1])])
        elif_ = st.tuples(conditions, conditions, block, block, block).map(
            lambda t: [
                f"if {t[0]}:",
                *_indent(t[2]),
                f"elif {t[1]}:",
                *_indent(t[3]),
                "else:",
                *_indent(t[4]),
            ]
        )
        sequence = st.lists(block, min_size=1, max_size=3).map(
            lambda blocks: [s for b in blocks for s in b]
        )
        return st.one_of(if_else, if_only, elif_, sequence)

    return st.recursive(simple, compound, max_leaves=12)


def _matches(block: st.SearchStrategy[list[str]]) -> st.SearchStrategy[list[str]]:
    return st.tuples(plain, block, block).map(
        lambda t: [
            f"match {t[0]}:",
            "    case True:",
            *_indent(_indent(t[1])),
            "    case _:",
            *_indent(_indent(t[2])),
        ]
    )


def _function(body: list[str]) -> str:
    lines = [
        "def f(a: bool, b: bool, c: bool) -> None:",
        '    q = "SELECT"',
        '    x = "v"',
        *_indent(body),
        "    run(q)",
    ]
    return "\n".join(lines) + "\n"


def _concrete(source: str) -> set[str]:
    found: set[str] = set()
    namespace: dict[str, object] = {"run": found.add}
    exec(compile(source, "<generated>", "exec"), namespace)
    for inputs in itertools.product((False, True), repeat=3):
        with contextlib.suppress(ValueError, UnboundLocalError):
            namespace["f"](*inputs)  # type: ignore[operator]
    return found


def _static(source: str) -> set[str] | Unchecked:
    parsed = ParsedSource(source)
    call = next(c for c in parsed.calls if isinstance(c.func, ast.Name) and c.func.id == "run")
    found = query_variants(parsed, call, call.args[0], limit=10**6)
    return found if isinstance(found, Unchecked) else {variant.query for variant in found}


def _compare(source: str, precise: bool) -> None:
    concrete = _concrete(source)
    static = _static(source)
    listing = textwrap.indent(source, "  ")
    if isinstance(static, Unchecked) and not concrete and static.reason is None:
        return  # `run` is never reached
    assert not isinstance(static, Unchecked), f"static code read as unchecked:\n{listing}"
    assert concrete <= static, f"missed {sorted(concrete - static)}:\n{listing}"
    if precise:
        assert static == concrete, f"infeasible {sorted(static - concrete)}:\n{listing}"


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=[HealthCheck.too_slow])
@given(_statements(plain))
def test_variants_are_exact_with_plain_conditions(body: list[str]) -> None:
    _compare(_function(body), precise=True)


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=[HealthCheck.too_slow])
@given(
    st.lists(
        st.one_of(_statements(st.one_of(plain, negated)), _matches(_statements(plain))),
        min_size=1,
        max_size=3,
    )
)
def test_variants_cover_every_query(blocks: list[list[str]]) -> None:
    _compare(_function([s for block in blocks for s in block]), precise=False)

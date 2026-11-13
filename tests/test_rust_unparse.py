"""``ast.unparse`` written in Rust (``sqlty_python::unparse``) against the one of Python.

Expressions of the package, fixed spellings and generated ones must unparse alike. Raise
the number of generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import os
import pathlib
import warnings

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty._core import unparse_expression

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))
ROOT = pathlib.Path(__file__).resolve().parent.parent
FUZZ = settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=list(HealthCheck))

# Assigned in every Unicode version the two sides may carry; newer characters may print
# escaped on one side only.
TEXT = st.text(
    st.sampled_from(
        "ab'\"\\\n\t\r {}%é\x00\x7f\x85\xa0\xad\u0378\u2028\u3000\ud7ff\ue000\U0001f600\U000e0001"
    ),
    max_size=6,
)
LEAVES = st.one_of(
    st.sampled_from(
        ["a", "b.c", "None", "True", "...", "1", "0x1f", "1_0", "1.5", "1e100", "1e-7"]
    ),
    st.sampled_from(["2j", "1.5e300j", "99999999999999999999", "0o17", "0b1", "1e999", ".5"]),
    TEXT.map(repr),
    TEXT.map(lambda text: "u" + repr(text)),
    TEXT.map(lambda text: repr(text.encode())),
    st.tuples(TEXT, TEXT).map(lambda p: f"{p[0]!r} {p[1]!r}"),
)


def _expressions(children: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
    pair = st.tuples(children, children)
    binary = st.sampled_from(
        [
            "+",
            "-",
            "*",
            "/",
            "//",
            "%",
            "@",
            "**",
            "<<",
            ">>",
            "|",
            "^",
            "&",
            "and",
            "or",
            "==",
            "!=",
            "<",
            "<=",
            ">",
            ">=",
            "is",
            "in",
        ]
        + ["is not", "not in"]
    )
    unary = st.sampled_from(["-", "+", "~", "not ", "await ", "*"])
    return st.one_of(
        st.tuples(children, binary, children).map(" ".join),
        st.tuples(children, binary, children).map(lambda p: f"({p[0]}) {p[1]} ({p[2]})"),
        st.tuples(unary, children).map("".join),
        st.tuples(unary, children).map(lambda p: f"{p[0]}({p[1]})"),
        pair.map(lambda p: f"{p[0]} if {p[1]} else {p[0]}"),
        pair.map(lambda p: f"({p[0]} if {p[1]} else {p[0]}) if {p[1]} else {p[0]}"),
        pair.map(lambda p: f"lambda x, /, y={p[0]}, *z, k, **kw: {p[1]}"),
        children.map("lambda: {}".format),
        pair.map(lambda p: f"{p[0]}[{p[1]}]"),
        pair.map(lambda p: f"{p[0]}[{p[1]}, {p[1]}]"),
        pair.map(lambda p: f"{p[0]}[{p[1]}:, ::{p[0]}]"),
        pair.map(lambda p: f"{p[0]}({p[1]}, *{p[1]}, k={p[0]}, **{p[1]})"),
        children.map("({}).attr".format),
        children.map("({},)".format),
        pair.map(lambda p: f"({p[0]}, {p[1]})"),
        pair.map(lambda p: f"[{p[0]}, *{p[1]}]"),
        pair.map(lambda p: f"{{{p[0]}, {p[1]}}}"),
        pair.map(lambda p: f"{{{p[0]}: {p[1]}, **{p[0]}}}"),
        pair.map(lambda p: f"[{p[0]} for x in {p[1]} if x async for y, z in w]"),
        pair.map(lambda p: f"{{{p[0]}: {p[1]} for x in y}}"),
        pair.map(lambda p: f"f({p[0]} for x in {p[1]})"),
        pair.map(lambda p: f"(x := {p[0]}, {p[1]})"),
        children.map("(yield {})".format),
        children.map("(yield from {})".format),
        pair.map(lambda p: f"f'{{{p[0]}!r:>{{{p[1]}}}}} {{x=}} {{{{'"),
        pair.map(lambda p: f'f"""{{{p[0]}}}\n{{ {p[1]} = :x}}"""'),
        pair.map(lambda p: f"t'{{{p[0]}!s}} {{{p[1]}:{{w}}.2f}}'"),
        children.map("f'{{ {} }}' 'tail'".format),
    )


EXPRESSIONS = st.recursive(LEAVES, _expressions, max_leaves=6)


@FUZZ
@given(EXPRESSIONS)
def test_generated_expressions(source: str) -> None:
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            expected = ast.unparse(ast.parse(source, mode="eval").body)
        except SyntaxError, ValueError:
            return
    assert unparse_expression(source) == expected, source
    assert unparse_expression(expected) == expected, source


def test_expressions_of_the_package() -> None:
    """Every expression of sqlty's own source, as written and as unparsed."""
    checked = 0
    for path in sorted((ROOT / "src" / "sqlty").rglob("*.py")):
        if path.name == "_stdlib.py":
            continue
        text = path.read_text()
        for node in ast.walk(ast.parse(text)):
            if isinstance(node, ast.expr) and not isinstance(node, (ast.Slice, ast.Starred)):
                expected = ast.unparse(node)
                written = ast.get_source_segment(text, node)
                assert written is not None
                for source in (f"({written}\n)", expected):
                    try:  # the text of an f-string is not an expression
                        if ast.unparse(ast.parse(source, mode="eval").body) != expected:
                            continue
                    except SyntaxError:
                        continue
                    if (found := unparse_expression(source)) is not None:
                        assert found == expected, source
                        checked += 1
    assert checked > 50_000


def expected_unparse(source: str) -> str | None:
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            return ast.unparse(ast.parse(source, mode="eval").body)
        except SyntaxError, ValueError:
            return None


SPELLINGS = [
    """t'{f'{a!r:>{"'"}} {{'!s}'""",
    "t'{ x != y  !r }'",
    "t'{a!r:{b!s}}'",
    "'''a\r\nb'''",
    "b'''a\rb'''",
    "r'''a\r\nb'''",
    "f'''{x:a\rb}'''",
    "t'''{a +\r\n b}'''",
    "t'''{a +\r b}'''",
    "f'''{x # c\n=}'''",
    "t'''{a # c\n}'''",
    "t'''{a +# c\n b}'''",
    "t'''{a + '#' # c\n}'''",
    "t'{x!=1:y}'",
    "t'{a!=b!=c:y}'",
    "t'{a if b!=c else d:>3}'",
    "f'{x!=1=:y}'",
    "t'{x!=1}'",
    "t'{x!=1!r}'",
    "t'{a[b!=c]:y}'",
    "t'{\"!\":y}'",
    "f'{x = !r}'",
    "t'{ x = }'",
    "967742817868973.25",
    "2126528249093299.25j",
    "9007199254740993.0",
    "5e-324",
    "1e16",
    "9" * 4300,
    "9" * 4301,
]


@pytest.mark.parametrize("source", SPELLINGS)
def test_spellings(source: str) -> None:
    assert unparse_expression(source) == expected_unparse(source)


@FUZZ
@given(st.floats(allow_nan=False, allow_infinity=False, min_value=0), st.booleans())
def test_generated_floats(value: float, imaginary: bool) -> None:
    source = repr(value) + "j" * imaginary
    assert unparse_expression(source) == expected_unparse(source), source


FIELD_PARTS = st.sampled_from(
    [
        "x",
        "a + b",
        "a != b",
        "a!=b",
        "d['k']",
        'd["!"]',
        "f(a, b)",
        "(a\n + b)",
        "a # c\n",
        "a +# '\n b",
    ]
    + [
        " x ",
        "x=",
        "x = ",
        "x!r",
        "x!s",
        "x:>3",
        "x!r:>3",
        "x=:y",
        "x=!a",
        "x:{w}",
        "x\r\n",
        "x:a\rb",
    ]
)


@FUZZ
@given(
    st.sampled_from(["f", "t", "F", "rf", "tr"]),
    st.sampled_from(["'''", '"""']),
    st.lists(
        st.one_of(
            st.sampled_from(["a", " ", "\r\n", "\r", "{{", "#", "'"]),
            FIELD_PARTS.map("{{{}}}".format),
        )
    ),
)
def test_generated_interpolations(prefix: str, quote: str, parts: list[str]) -> None:
    source = f"{prefix}{quote}{''.join(parts)}{quote}"
    if (expected := expected_unparse(source)) is not None:
        assert unparse_expression(source) == expected, source

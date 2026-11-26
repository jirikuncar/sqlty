"""``--sql`` t-strings read in Rust (``sqlty_python::templates``).

Raise the number of generated examples with ``SQLTY_FUZZ_EXAMPLES``
under ``--hypothesis-profile=fuzz``.
"""

from __future__ import annotations

import ast
import os
import re

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty import SQLTemplate
from sqlty._core import template_strings

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))
MARKER = re.compile(r"\s*--\s?sql(?:\s|$)")


def marked(text: str) -> list[tuple[str, list[str | None]]]:
    """The query and parameter types of each ``--sql`` t-string whose fields all pass values."""
    return [
        (query, list(params))
        for _, _, query, params in template_strings(text) or []
        if query is not None and MARKER.match(query)
    ]


CASES = {
    "fields and specs": (
        'def f(n: int, s: str):\n    q = t"--sql\\nSELECT {n}, {s:b}, {1.5:t}, {x:s}, 5%"\n',
        [("--sql\nSELECT %s, %s, %s, %s, 5%%", ["int", "str", "float", None])],
    ),
    "debug text is part of the query": (
        'q = t"--sql\\nSELECT {n=:s}"\n',
        [("--sql\nSELECT n=%s", [None])],
    ),
    "debug text without a spec converts": ('q = t"--sql\\nSELECT {n=}"\n', []),
    "conversion": ('q = t"--sql\\nSELECT {n!r}"\n', []),
    "nested spec": ('q = t"--sql\\nSELECT {n:{w}}"\n', []),
    "empty spec": ('q = t"--sql\\nSELECT {n:}"\n', []),
    "concatenated": (
        'q = (t"--sql\\nSELECT {1}"\n     t" AND {2}")\n',
        [("--sql\nSELECT %s AND %s", ["int", "int"])],
    ),
    "unmarked": ('q = t"SELECT {1}"\nr = "--sql"\n', []),
    "nested in a field": ("q = t\"{t'--sql\\nSELECT {1}'}\"\n", [("--sql\nSELECT %s", ["int"])]),
    "triple-quoted": ('q = t"""--sql\nSELECT {1}\n"""\n', [("--sql\nSELECT %s\n", ["int"])]),
    "unparsable": ('q = t"--sql\nSELECT {1}"\ndef (\n', []),
}


@pytest.mark.parametrize(("text", "expected"), CASES.values(), ids=CASES.keys())
def test_cases(text: str, expected: list[tuple[str, list[str | None]]]) -> None:
    for newline in ("\n", "\r\n", "\r"):
        assert marked(text.replace("\n", newline)) == expected


FIELDS = st.sampled_from(
    [
        "{n}",
        "{s:b}",
        "{x:t}",
        "{1}",
        "{'a'}",
        "{n=:s}",
        "{n=}",
        "{n!r}",
        "{n:{w}}",
        "{n:}",
        "{ n }",
        "{(n, s)}",
    ]
)
TEXT = st.sampled_from(["SELECT ", "%", " AND ", "\\n", "{{", "}}", "é", "\n"])


@st.composite
def template(draw: st.DrawFn) -> str:
    parts = []
    for i in range(draw(st.integers(1, 2))):
        quote = draw(st.sampled_from(['"""', "'''"]))
        head = "--sql\n" if i == 0 and draw(st.booleans()) else ""
        body = "".join(draw(st.lists(st.one_of(FIELDS, TEXT), max_size=5)))
        parts.append(f"{draw(st.sampled_from(['t', 'T', 'rt', 'Tr']))}{quote}{head}{body}{quote}")
    return "(" + " ".join(parts) + ")"


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=list(HealthCheck))
@given(
    st.lists(
        st.tuples(template(), st.sampled_from(["module", "def", "method"])), min_size=1, max_size=4
    ),
    st.sampled_from(["\n", "\r\n", "\r"]),
)
def test_generated_modules(templates: list[tuple[str, str]], newline: str) -> None:
    lines = ["n = 1", "s: str = f()"]
    for text, scope in templates:
        match scope:
            case "def":
                lines += ["def f(n: float, w):", f"    q = {text}"]
            case "method":
                lines += ["class C:", "    def m(self, s: bytes):", f"        q = {text}"]
            case _:
                lines.append(f"q = {text}")
    source = "\n".join([*lines, ""]).replace("\n", newline)
    try:
        ast.parse(source)
    except SyntaxError:
        return
    for query, params in marked(source):
        assert query.count("%s") == len(params), source


def test_repr_shows_the_template() -> None:
    user_id = 1
    query = SQLTemplate[tuple[int]](t"--sql\nSELECT {user_id}")
    assert repr(query) == f"SQLTemplate({query.template!r})"

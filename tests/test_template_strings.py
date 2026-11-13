"""``--sql`` t-strings read in Rust (``sqlty_python::templates``) against ``ast``.

Raise the number of generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import os

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

import sqlty._drivers
from sqlty._drivers import find_template_calls
from sqlty._helpers import HelperCall
from sqlty._source import ParsedSource

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))


def both(text: str, monkeypatch: pytest.MonkeyPatch) -> list[HelperCall]:
    monkeypatch.setattr(sqlty._drivers, "AST", "rust")
    rust = find_template_calls(ParsedSource(text))
    monkeypatch.setattr(sqlty._drivers, "AST", "python")
    python = [call._replace(query_node=None) for call in find_template_calls(ParsedSource(text))]
    assert rust == python, text
    return rust


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
}


@pytest.mark.parametrize(("text", "expected"), CASES.values(), ids=CASES.keys())
def test_cases(
    text: str, expected: list[tuple[str, list[str | None]]], monkeypatch: pytest.MonkeyPatch
) -> None:
    for newline in ("\n", "\r\n", "\r"):
        found = both(text.replace("\n", newline), monkeypatch)
        assert [(c.query, [p.type for p in c.params or ()]) for c in found] == expected


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
    with pytest.MonkeyPatch.context() as monkeypatch:
        both(source, monkeypatch)

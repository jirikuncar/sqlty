"""``--sql`` t-strings as ``sqlty_python::templates`` reads them: the query a driver runs
and the type of each field."""

from __future__ import annotations

import re

import pytest

from sqlty._core import template_strings

MARKER = re.compile(r"\s*--\s?sql(?:\s|$)")


def marked(text: str) -> list[tuple[str, list[str | None]]]:
    found = template_strings(text)
    assert found is not None, text
    return [
        (query, params)
        for _, _, query, params in found
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
}


@pytest.mark.parametrize(("text", "expected"), CASES.values(), ids=CASES.keys())
@pytest.mark.parametrize("newline", ["\n", "\r\n", "\r"])
def test_cases(text: str, expected: list[tuple[str, list[str | None]]], newline: str) -> None:
    assert marked(text.replace("\n", newline)) == expected

"""``sqlty_python::suppress`` reads ``# sqlty: ignore`` comments as ``ParsedSource.ignores`` does."""

from __future__ import annotations

import os
import sys

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from sqlty._core import module_ignores
from sqlty._source import ParsedSource

VERSION = sys.version_info[:2]
EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))


def same(source: str) -> int:
    """The number of ignore comments, after checking that Rust reads the same ones."""
    python = [
        (
            ignore.line,
            None if ignore.rules is None else list(ignore.rules),
            ignore.start,
            ignore.end,
        )
        for ignore in ParsedSource(source).ignores.values()
    ]
    rust = module_ignores(source, VERSION)
    if rust is None:
        # Rust reads no file that does not parse; `tokenize` reads it up to the error.
        with pytest.raises(SyntaxError):
            compile(source, "<source>", "exec")
        return -1
    assert rust == python, source
    return len(python)


CASES = [
    ("x = 1  # sqlty: ignore\n", 1),
    ("x = 1  #sqlty:ignore[a, b,,  c ]\n", 1),
    ("x = 1  # sqlty: ignore[]\n", 1),
    ("x = 1  # sqlty: ignore[a\n", 1),
    ("x = 1  # sqlty: ignored\n", 0),
    ("x = 1  # sqlty: ignore_all\n", 0),
    ("x = 1  # sqlty: ignoreé\n", 0),
    ("x = 1  # sqlty: ignore-this\n", 1),
    ("x = 1  # note # sqlty: ignore\n", 0),
    ("x = 1  ## sqlty: ignore\n", 0),
    ("x = 1  #\t\x0c sqlty:\x1f ignore[\x1ca\x1d]\n", 1),
    ("x = 1  #\xa0sqlty:\u2003ignore[\u3000a]\n", 1),
    ("x = '# sqlty: ignore'\n", 0),
    ('x = """\n# sqlty: ignore\n"""\n', 0),
    ("x = f'{1  # sqlty: ignore\n}'\n", 1),
    ("é = 'é'  # sqlty: ignore[é]\ny = 2  # sqlty: ignore\n", 2),
    ("x = 1 \\\n  + 2  # sqlty: ignore\r\ny = 2  # sqlty: ignore\r\nz = 3  # sqlty: ignore", 3),
    ("x = (\n    1,  # sqlty: ignore[a]\n    2,  # sqlty: ignore[a][b]\n)\n", 2),
    ("if x:\n    pass\n# sqlty: ignore\n\x0c# sqlty: ignore[a]\n", 2),
]


@pytest.mark.parametrize(("source", "count"), CASES)
def test_cases(source: str, count: int) -> None:
    assert same(source) == count


BLANK = st.sampled_from(["", " ", "  ", "\t", "\x0c", "\x1f", "\xa0", "\u2003"])
NAME = st.sampled_from(["a", "unknown-column", "é", "a b", "", " ", "x]", "[", "#", "'"])
TAIL = st.sampled_from(["", "d", "_", " more", "[", "]", "-x", "é", "# sqlty: ignore", ".", "0"])


@st.composite
def comments(draw: st.DrawFn) -> str:
    rules = ""
    if draw(st.booleans()):
        rules = "[" + ",".join(draw(st.lists(NAME, max_size=3))) + draw(st.sampled_from(["]", ""]))
    word = draw(st.sampled_from(["ignore", "ignore", "Ignore", "ignor"]))
    return f"#{draw(BLANK)}sqlty:{draw(BLANK)}{word}{rules}{draw(TAIL)}"


@st.composite
def lines(draw: st.DrawFn) -> str:
    comment = draw(comments())
    shape = draw(
        st.sampled_from(
            [
                "x = 1  {}",
                "{}",
                "    {}",
                "x = '{}'",
                'x = """\n{}\n"""',
                "x = (\n    1,  {}\n    2,\n)",
                "x = f'{{1  {}\n}}'",
                "def f():\n    return 1  {}",
                "é = 'é'  {}",
                "x = 1 \\\n    + 2  {}",
                "x = [  {}\n    y for y in z]",
                "x = 1  # note {}",
            ]
        )
    )
    return shape.format(comment)


def test_lone_carriage_returns_are_lines_in_rust_only() -> None:
    # `tokenize` stops at a lone CR, which `ast` and ruff both take for a line break.
    source = "x = 1  # sqlty: ignore\ry = 2  # sqlty: ignore\n"
    assert [found[0] for found in module_ignores(source, VERSION) or []] == [1, 2]
    assert list(ParsedSource(source).ignores) == [1]


@settings(max_examples=EXAMPLES, deadline=None)
@given(st.lists(lines(), min_size=1, max_size=6), st.sampled_from(["\n", "\r\n"]))
def test_generated_comments(parts: list[str], newline: str) -> None:
    same("\n".join(parts).replace("\n", newline) + newline)

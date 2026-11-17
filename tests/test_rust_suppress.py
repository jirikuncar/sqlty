"""``sqlty_python::suppress`` reads ``# sqlty: ignore`` comments as ``tokenize`` finds them."""

from __future__ import annotations

import io
import os
import re
import sys
import tokenize

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from sqlty._core import module_ignores

VERSION = sys.version_info[:2]
EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))


IGNORE = re.compile(r"#\s*sqlty:\s*ignore\b(?:\[([^\]]*)\])?")


def tokenized(source: str) -> list[tuple[int, list[str] | None, int, int]]:
    """The ignore comments of the COMMENT tokens of ``source``, with their byte offsets."""
    starts = [0]
    for line in source.encode().splitlines(keepends=True):
        starts.append(starts[-1] + len(line))
    found = []
    for token in tokenize.generate_tokens(io.StringIO(source).readline):
        if token.type != tokenize.COMMENT or (match := IGNORE.match(token.string)) is None:
            continue
        names = match.group(1)
        rules = None if names is None else [n.strip() for n in names.split(",") if n.strip()]
        line, column = token.start
        start = starts[line - 1] + len(token.line[:column].encode())
        found.append((line, rules, start, start + len(token.string.encode())))
    return found


def same(source: str) -> int:
    """The number of ignore comments, after checking that Rust reads the same ones."""
    rust = module_ignores(source, VERSION)
    try:
        compile(source, "<source>", "exec")
    except SyntaxError:
        # `tokenize` stops at what does not lex; Rust reads every comment the lexer finds.
        return len(rust)
    python = tokenized(source)
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
    # Sources that do not parse: every comment outside a string.
    ("x = (  # sqlty: ignore\ny = 2  # sqlty: ignore[a]\n", 2),
    ("def f(:  # sqlty: ignore\n", 1),
    ("x = 1  # sqlty: ignore\ny = 'open  # sqlty: ignore\nz = 3  # sqlty: ignore\n", 2),
    ('x = 1  # sqlty: ignore\ny = """open\n# sqlty: ignore\n', 1),
    ("if x:\n        a = 1  # sqlty: ignore\n    b = 2  # sqlty: ignore\n", 2),
    ("x = 1  # sqlty: ignore\ny = $  # sqlty: ignore\n", 2),
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


@settings(max_examples=EXAMPLES, deadline=None)
@given(st.lists(lines(), min_size=1, max_size=6), st.sampled_from(["\n", "\r\n"]))
def test_generated_comments(parts: list[str], newline: str) -> None:
    same("\n".join(parts).replace("\n", newline) + newline)

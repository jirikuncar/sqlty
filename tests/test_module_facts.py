"""Differential test of ``sqlty_python::module``: what Rust reads of a module against ``ast``.

Generated modules nest imports, aliases and calls in every block kind; the corpus is every
Python file of the repository and every Python block of the Markdown tests. Raise the number
of generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import os
import re
from pathlib import Path

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty._core import module_facts
from sqlty._source import ParsedSource

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))
ROOT = Path(__file__).parent.parent
WRAPPERS = ("SQL", "sqlty.SQL", "Q", "x.SQL")


def python_facts(text: str) -> tuple[set[str], list[tuple[str, str]], dict[str, list[int]]]:
    parsed = ParsedSource(text)
    assert parsed.tree is not None
    called = {
        node.func.id if isinstance(node.func, ast.Name) else node.func.attr
        for node in ast.walk(parsed.tree)
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name | ast.Attribute)
    }
    imports = sorted(
        (alias.asname or alias.name, alias.name)
        for node in parsed.imports
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
    )
    return called, imports, {wrapper: parsed._any_calls(wrapper) for wrapper in WRAPPERS}


def assert_same(text: str) -> None:
    try:
        ast.parse(text)
    except SyntaxError, ValueError:
        return
    rust = module_facts(text)
    assert rust is not None, f"ruff rejects what ast parses:\n{text}"
    called, from_imports, any_calls = rust
    python_called, python_imports, python_any = python_facts(text)
    assert set(called) == python_called, text
    assert sorted(from_imports) == python_imports, text
    for wrapper in WRAPPERS:
        found = [o for o, name in any_calls if name == wrapper or name.endswith(f".{wrapper}")]
        assert found == python_any[wrapper], (wrapper, text)


IMPORTS = [
    "from typing import Any",
    "from typing import Any as A",
    "from typing import List as A",
    "from typing_extensions import Annotated as Ann, Any",
    "from typing import Annotated",
    "import typing as t",
    "import typing_extensions as t",
    "import os as t",
    "from m import run as r, fetch",
    "from . import run",
    "from .m import run as A",
]
SLICES = [
    "Any",
    "A",
    "t.Any",
    "'Any'",
    '"A"',
    "' Any'",
    "'t.Any'",
    "'Any'  'x'",
    "List",
    "int",
    "Ann[Any, 1]",
    "Annotated[A, 'x']",
    "t.Annotated[t.Any, 1]",
    "Annotated[int, Any]",
    "Annotated[Any]",
    "'Annotated[Any, 1]'",
    "tuple[Any]",
    "Any | None",
    "'('",
]
CALLEES = ["SQL", "sqlty.SQL", "Q", "x.SQL", "f().SQL", "a[0].SQL", "x.y.SQL", "(SQL)"]
expressions = st.one_of(
    st.builds(
        lambda w, s: f"{w}[{s}]('--sql\\nSELECT 1')",
        st.sampled_from(CALLEES),
        st.sampled_from(SLICES),
    ),
    st.sampled_from(
        ["run(1)", "r(x)", "fetch()", "db.fetch_all(q)", "x.y.run()", "f()(1)", "(lambda: r())()"]
    ),
)


def _wrap(body: list[str], kind: str) -> list[str]:
    inner = [f"    {line}" for line in body] or ["    pass"]
    match kind:
        case "if":
            return ["if c:", *inner, "elif d:", *inner, "else:", *inner]
        case "try":
            return ["try:", *inner, "except E:", *inner, "finally:", *inner]
        case "def":
            return ["@deco(run(1))", "def f(a=r()):", *inner]
        case "class":
            return ["class C:", *inner]
        case "match":
            return ["match x:", "    case 1:", *[f"    {line}" for line in inner]]
        case "with":
            return ["with r() as w:", *inner]
        case "for":
            return ["for i in y:", *inner, "else:", *inner]
    return body


statements = st.one_of(
    st.sampled_from(IMPORTS).map(lambda line: [line]),
    expressions.map(lambda e: [f"q = {e}"]),
    expressions.map(lambda e: [f"v = f'{{{e}}}'"]),
    expressions.map(lambda e: [f"w = [{e} for _ in y]"]),
    st.just(["s = 'žluťoučký kůň'  # ünïcode"]),
)
blocks = st.recursive(
    st.lists(statements, min_size=1, max_size=4).map(
        lambda parts: [line for part in parts for line in part]
    ),
    lambda inner: st.builds(
        lambda body, kind, rest: _wrap(body, kind) + rest,
        inner,
        st.sampled_from(["if", "try", "def", "class", "match", "with", "for"]),
        inner,
    ),
    max_leaves=12,
)


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=[HealthCheck.too_slow])
@given(blocks, st.sampled_from(["\n", "\r\n"]))
def test_generated_modules(lines: list[str], newline: str) -> None:
    assert_same(newline.join(lines) + newline)


def _corpus() -> list[tuple[str, str]]:
    found = [
        (str(path.relative_to(ROOT)), path.read_text())
        for path in sorted(ROOT.glob("**/*.py"))
        if ".venv" not in path.parts and "target" not in path.parts
    ]
    for path in sorted((ROOT / "tests" / "mdtest").glob("**/*.md")):
        for index, block in enumerate(
            re.findall(r"```python\n(.*?)```", path.read_text(), re.DOTALL)
        ):
            found.append((f"{path.relative_to(ROOT)}#{index}", block))
    return found


@pytest.mark.parametrize(
    ("name", "text"), _corpus(), ids=lambda value: value if "/" in value else ""
)
def test_corpus(name: str, text: str) -> None:
    assert_same(text)

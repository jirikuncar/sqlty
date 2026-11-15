"""No test repeats another: a copy costs run time and says nothing new.

An mdtest under ``tests/`` is a copy when another section reads the same input and
asserts at least what it does. Documentation pages may overlap: they are read, not run
for coverage. A Python test is a copy when its decorators, signature and body match.
"""

from __future__ import annotations

import ast
import json
from collections import Counter, defaultdict
from pathlib import Path

import pytest

from tests._mdtest.parser import Section, parse

ROOT = Path(__file__).resolve().parent.parent
TESTS = ROOT / "tests"

Asserted = Counter[tuple[str, str, str | None]]


def _input(section: Section) -> str:
    units = [(unit.lang, unit.code.text) for unit in section.units]
    setup = [text for _, text in section.setup]
    state = [section.config, section.files, section.directives, units, setup]
    return json.dumps(state, sort_keys=True, default=str)


def _where(section: Section) -> str:
    return f"{section.path.relative_to(ROOT)}:{section.line} {section.name}"


def test_no_mdtest_repeats_another(pytestconfig: pytest.Config) -> None:
    paths: list[Path] = []
    for root in pytestconfig.getini("mdtest_paths"):
        paths += [root] if root.is_file() else sorted(root.rglob("*.md"))
    readers: defaultdict[str, list[tuple[Section, Asserted]]] = defaultdict(list)
    for path in paths:
        for section in parse(path):
            if section.units:
                asserted: Asserted = Counter(
                    (a.kind, a.expected, a.range) for a in section.assertions
                )
                readers[_input(section)].append((section, asserted))
    copies = [
        f"{_where(section)} repeats {_where(other)}"
        for same in readers.values()
        for i, (section, asserted) in enumerate(same)
        if TESTS in section.path.parents
        for j, (other, more) in enumerate(same)
        if i != j
        and not asserted - more
        and (asserted != more or TESTS not in other.path.parents or j < i)
    ]
    assert not copies, "\n".join(copies)


def _shape(test: ast.FunctionDef | ast.AsyncFunctionDef) -> str:
    body = test.body[1:] if ast.get_docstring(test) else test.body
    return ast.dump(ast.Module([*test.decorator_list, test.args, *body], []))  # type: ignore[list-item]


def test_no_python_test_repeats_another() -> None:
    tests: defaultdict[str, list[str]] = defaultdict(list)
    for path in sorted(TESTS.rglob("test_*.py")):
        for fn in ast.walk(ast.parse(path.read_text())):
            if isinstance(fn, ast.FunctionDef | ast.AsyncFunctionDef) and fn.name[:4] == "test":
                tests[_shape(fn)].append(f"{path.relative_to(ROOT)}:{fn.lineno} {fn.name}")
    copies = [" = ".join(same) for same in tests.values() if len(same) > 1]
    assert not copies, "\n".join(copies)

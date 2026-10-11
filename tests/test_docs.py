"""Run mdtest assertions embedded in README.md and docs/**/*.md.

Ensures that code examples shown in the documentation stay in sync with
the actual behaviour of sqlty's rewriter, type inference, and codec.
Files without assertion comments contribute no test cases.

Note: TOML config blocks apply to every later heading in the same file,
not only to child headings, so a section inherits ``[tool.sqlty]`` settings
(e.g. ``schema``) from earlier sibling sections. ``# decoded:`` ignores
config, but a new ``# rewritten:`` or ``# inferred:`` assertion added after a
TOML block may pick it up; repeat or reset the config in that section if it
matters.

Run with ``--update-mdtest`` to rewrite assertion comments in place.

``>>>`` examples in the pages listed in ``_DOCTEST_FILES`` run as doctests.
"""

from __future__ import annotations

import doctest
from pathlib import Path

import pytest

from tests.mdtest_runner import MDFileUpdater, MDTestCase, parse_mdtest_file

_ROOT = Path(__file__).resolve().parent.parent
# README.md is required: a missing file must error, not yield zero cases.
_FILES = [_ROOT / "README.md", *sorted((_ROOT / "docs").rglob("*.md"))]
_CASES = [case for path in _FILES for case in parse_mdtest_file(path)]
_UPDATERS: dict[Path, MDFileUpdater] = {}


@pytest.mark.parametrize(
    "case", _CASES, ids=[f"{c.md_path.relative_to(_ROOT)}::{c.name}" for c in _CASES]
)
def test_docs(case: MDTestCase, request: pytest.FixtureRequest) -> None:
    if request.config.getoption("--update-mdtest"):
        case.update = True
        case.updater = _UPDATERS.setdefault(case.md_path, MDFileUpdater(case.md_path))
    case.run()


_DOCTEST_FILES = [_ROOT / "docs" / "reference" / "python-api.md"]


@pytest.mark.parametrize(
    "path", _DOCTEST_FILES, ids=[str(p.relative_to(_ROOT)) for p in _DOCTEST_FILES]
)
def test_docs_doctest(path: Path) -> None:
    # Blank out code fences so a closing ``` is not read as expected output.
    text = "\n".join(
        "" if line.lstrip().startswith("```") else line for line in path.read_text().splitlines()
    )
    test = doctest.DocTestParser().get_doctest(text, {}, path.name, str(path), 0)
    runner = doctest.DocTestRunner(optionflags=doctest.ELLIPSIS)
    result = runner.run(test)
    assert result.attempted > 0
    assert result.failed == 0

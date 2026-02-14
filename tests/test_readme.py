"""Run mdtest assertions embedded in README.md.

Ensures that code examples shown in the README stay in sync with
the actual behaviour of sqlty's rewriter, type inference, and codec.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from tests.mdtest_runner import parse_mdtest_file

_README = Path(__file__).resolve().parent.parent / "README.md"
_CASES = parse_mdtest_file(_README) if _README.exists() else []


@pytest.mark.parametrize("case", _CASES, ids=[c.name for c in _CASES])
def test_readme(case):
    case.run()

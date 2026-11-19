"""Every lint rule is asserted by at least one ``# error: [rule]`` of an mdtest.

The assertions are read statically from the Markdown files of ``mdtest_paths`` in
``pyproject.toml``, so a rule that no test reports, or that a change stops reporting
everywhere, fails here.
"""

from __future__ import annotations

import re
import tomllib
from pathlib import Path

from sqlty._lint import RULES

ROOT = Path(__file__).resolve().parent.parent

# Rules no mdtest can assert, with the reason.
EXCLUDED = {
    # Reported when sqlty itself panics on a file; no input is meant to do that.
    "internal-error": "a crash of sqlty, tested in the engine's unit tests",
}

ASSERTION = re.compile(r"(?:#|--) error: \[(?P<rule>[a-z0-9-]+)\]")


def asserted() -> set[str]:
    config = tomllib.loads((ROOT / "pyproject.toml").read_text())
    paths = config["tool"]["pytest"]["ini_options"]["mdtest_paths"]
    files: list[Path] = []
    for entry in paths:
        path = ROOT / entry
        files.extend(sorted(path.rglob("*.md")) if path.is_dir() else [path])
    return {
        match["rule"]
        for file in files
        for match in ASSERTION.finditer(file.read_text(encoding="utf-8"))
    }


def test_every_rule_is_asserted_by_an_mdtest() -> None:
    missing = set(RULES) - asserted() - set(EXCLUDED)
    assert not missing, (
        f"no mdtest asserts {sorted(missing)}; add one, or exclude the rule with a reason"
    )


def test_an_excluded_rule_exists_and_is_not_asserted() -> None:
    hit = asserted()
    for name, reason in EXCLUDED.items():
        assert name in RULES and reason
        assert name not in hit, f"{name} is asserted now; drop its exclusion"

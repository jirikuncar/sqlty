"""Packaging metadata: published pre-commit hooks and the wheel's startup hook."""

from __future__ import annotations

import shlex
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

import pytest
import yaml

ROOT = Path(__file__).parent.parent
HOOKS: list[dict[str, Any]] = yaml.safe_load((ROOT / ".pre-commit-hooks.yaml").read_text())


@pytest.mark.parametrize("hook", HOOKS, ids=[h["id"] for h in HOOKS])
def test_pre_commit_hook_runs(hook: dict[str, Any], tmp_path: Path) -> None:
    """Each published hook runs the CLI the way pre-commit does: entry + args + filenames."""
    entry = shlex.split(hook["entry"])
    assert entry[0] == "sqlty"
    assert hook["language"] == "python"
    assert hook["types"] == ["python"]

    (tmp_path / "pyproject.toml").write_text("[tool.sqlty]\n")
    sample = tmp_path / "queries.py"
    sample.write_text('q = "--sql\\nSELECT id::INTEGER FROM users"\n')

    filenames = [str(sample)] if hook.get("pass_filenames", True) else []
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", *entry[1:], *hook.get("args", []), *filenames],
        capture_output=True,
        text=True,
        cwd=tmp_path,
    )
    # argparse errors exit with 2; rewrite/check exit 0 or 1.
    assert result.returncode in (0, 1), result.stderr
    assert "usage:" not in result.stderr


def test_wheel_includes_magic_pth() -> None:
    """maturin ships sqlty_magic.pth at the wheel root (site-packages) via `include`."""
    pyproject = tomllib.loads((ROOT / "pyproject.toml").read_text())
    assert "sqlty_magic.pth" in pyproject["tool"]["maturin"]["include"]
    assert (ROOT / "sqlty_magic.pth").read_text().strip() == "import sqlty.magic"

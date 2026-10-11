"""Packaging: published pre-commit hooks and the wheel's ``sqlty_magic.pth`` startup hook.

The built wheel and sdist are inspected in CI (``package`` job in
``.github/workflows/tests.yml`` and the release workflow); these tests cover
the hook entries and what the ``.pth`` line does at interpreter startup.
"""

from __future__ import annotations

import os
import shlex
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

import pytest
import yaml

ROOT = Path(__file__).parent.parent
PTH = ROOT / "src" / "sqlty_magic.pth"
HOOKS: list[dict[str, Any]] = yaml.safe_load((ROOT / ".pre-commit-hooks.yaml").read_text())

# Expected outcome per hook for an unwrapped, castable query.
HOOK_RESULTS = {
    # rewrite edits the file in place and exits 0; pre-commit fails the commit
    # because the file changed
    "sqlty-rewrite": (0, True),
    "sqlty": (0, True),
    # check reports the unwrapped string as stale-rewrite, a warning: exit 0
    "sqlty-check": (0, False),
}


def test_every_hook_has_expected_result() -> None:
    assert {h["id"] for h in HOOKS} == set(HOOK_RESULTS)


@pytest.mark.parametrize("hook", HOOKS, ids=[h["id"] for h in HOOKS])
def test_pre_commit_hook_runs(hook: dict[str, Any], tmp_path: Path) -> None:
    """Each published hook runs the CLI the way pre-commit does: entry + args + filenames."""
    entry = shlex.split(hook["entry"])
    assert entry[0] == "sqlty"
    assert hook["language"] == "python"
    assert hook["types"] == ["python"]
    assert hook.get("pass_filenames", True)

    (tmp_path / "pyproject.toml").write_text("[tool.sqlty]\n")
    sample = tmp_path / "queries.py"
    sample.write_text('q = "--sql\\nSELECT id::INTEGER FROM users"\n')

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", *entry[1:], *hook.get("args", []), str(sample)],
        capture_output=True,
        text=True,
        cwd=tmp_path,
    )
    returncode, rewritten = HOOK_RESULTS[hook["id"]]
    assert result.returncode == returncode, result.stdout + result.stderr
    text = sample.read_text()
    assert ('SQL[tuple[int]]("--sql' in text) is rewritten, text
    assert ("from sqlty import SQL" in text) is rewritten, text


def test_deprecated_hook_id_says_so() -> None:
    """pre-commit prints `name` on every run but never `description`."""
    (alias,) = (h for h in HOOKS if h["id"] == "sqlty")
    assert "deprecated" in alias["name"]
    assert "sqlty-rewrite" in alias["name"]


def test_maturin_ships_magic_pth() -> None:
    """The include path is project-root relative, which needs maturin >= 1.12.1.

    Older maturin globbed wheel includes relative to ``python-source``, so this
    path would silently be missing from the wheel.
    """
    pyproject = tomllib.loads((ROOT / "pyproject.toml").read_text())
    assert pyproject["tool"]["maturin"]["include"] == ["src/sqlty_magic.pth"]
    assert pyproject["build-system"]["requires"] == ["maturin>=1.12.1,<2.0"]
    assert PTH.is_file()


def _startup(site_dir: Path, code: str, **env: str) -> subprocess.CompletedProcess[str]:
    """Run ``code`` after processing ``site_dir`` the way ``site`` handles site-packages."""
    environ = {k: v for k, v in os.environ.items() if k != "SQLTY_NO_MAGIC"} | env
    script = f"import site, sys\nsite.addsitedir({str(site_dir)!r})\n{code}"
    return subprocess.run(
        [sys.executable, "-B", "-c", script],
        capture_output=True,
        text=True,
        cwd=site_dir,
        env=environ,
    )


@pytest.fixture
def site_dir(tmp_path: Path) -> Path:
    shutil.copy(PTH, tmp_path / PTH.name)
    (tmp_path / "coded.py").write_text(
        '# coding: sqlty\nq = "--sql\\nSELECT 1::INTEGER AS x"\nprint(type(q).__name__)\n'
    )
    return tmp_path


def test_magic_pth_does_not_import_sqlty_at_startup(site_dir: Path) -> None:
    """The .pth line runs in every interpreter, so it must not import sqlty (#108)."""
    result = _startup(
        site_dir,
        "import codecs\n"
        "codecs.lookup('utf-8')\n"
        "print(sorted(m for m in sys.modules if m.split('.')[0] == 'sqlty'))\n"
        "print('importlib.metadata' in sys.modules)",
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.split() == ["[]", "False"]


def test_magic_pth_registers_codec_lazily(site_dir: Path) -> None:
    result = _startup(site_dir, "import coded\nprint('sqlty.magic' in sys.modules)")
    assert result.returncode == 0, result.stderr
    assert result.stdout.split() == ["SQL", "True"]


def test_magic_pth_respects_sqlty_no_magic(site_dir: Path) -> None:
    result = _startup(
        site_dir,
        "import codecs\n"
        "try:\n"
        "    codecs.lookup('sqlty')\n"
        "except LookupError:\n"
        "    print('unknown')\n"
        "print('sqlty' in sys.modules)",
        SQLTY_NO_MAGIC="1",
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.split() == ["unknown", "False"]


Q = 'q = "--sql\\nSELECT 1::INTEGER AS x"\n'
DT = "q = \"--sql\\nSELECT NOW()::TIMESTAMP AS t, 'a'::UUID AS u\"\n"


@pytest.mark.parametrize(
    ("source", "shift"),
    [
        pytest.param("# coding: sqlty\n" + Q, 0, id="no-docstring"),
        pytest.param('# coding: sqlty\n"""Doc."""\n' + Q, 0, id="docstring"),
        pytest.param("# coding: sqlty\nfrom __future__ import annotations\n" + Q, 0, id="future"),
        pytest.param("#!/usr/bin/env python\n# coding: sqlty\n" + Q, 0, id="shebang"),
        pytest.param("\n# coding: sqlty\n" + Q, 0, id="blank-before-cookie"),
        pytest.param("# coding: sqlty\nimport sys\nfrom sqlty import SQL\n" + Q, 0, id="import"),
        pytest.param("# coding: sqlty\n\n@staticmethod\ndef f(): ...\n" + Q, 0, id="decorator"),
        pytest.param("# coding: sqlty\ndef f(): ...\n" + Q, 1, id="def-after-cookie"),
        pytest.param("# coding: sqlty\n" + DT, 0, id="datetime-uuid"),
        pytest.param('# coding: sqlty\r\n"""Doc."""\r\n' + DT.replace("\n", "\r\n"), 0, id="crlf"),
    ],
)
def test_magic_pth_runs_coded_script(site_dir: Path, source: str, shift: int) -> None:
    """``python script.py`` decodes from the end of the cookie line and drops the first line (#111)."""
    (site_dir / "sitecustomize.py").write_text(f"import site\nsite.addsitedir({str(site_dir)!r})\n")
    newline = "\r\n" if "\r\n" in source else "\n"
    source += f"import sys{newline}print(type(q).__name__, sys._getframe().f_lineno){newline}"
    script = site_dir / "script.py"
    script.write_bytes(source.encode())
    environ = {k: v for k, v in os.environ.items() if k != "SQLTY_NO_MAGIC"}
    result = subprocess.run(
        [sys.executable, "-B", str(script)],
        capture_output=True,
        text=True,
        cwd=site_dir,
        env=environ | {"PYTHONPATH": str(site_dir)},
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.split() == ["SQL", str(source.count("\n") + shift)]

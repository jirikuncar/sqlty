"""E2E tests for ``sqlty check --fix`` and the fstring-skipped / tstring-skipped warnings."""

import importlib.util
import os
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n"

STALE = """\
from sqlty import SQL
a = SQL[tuple[str, str]]("--sql\\nSELECT id, name FROM users")
b = SQL[tuple[int, str]]("--sql\\nSELECT * FROM users")
c = "--sql\\nSELECT id::INTEGER FROM users"
"""

FIXED = """\
from sqlty import SQL
a = SQL[tuple[int, str]]("--sql\\nSELECT id, name FROM users")
b = SQL[tuple[int, str]]("--sql\\nSELECT id::INT, name::TEXT FROM users")
c = SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM users")
"""

SKIPPED = """\
table = "users"
a = f"--sql\\nSELECT id FROM {table}"
b = t"--sql\\nSELECT id FROM {table}"
"""

FSTRING = "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
TSTRING = "t-string with --sql prefix skipped (t-strings cannot be statically rewritten)"

Run = Callable[..., subprocess.CompletedProcess[str]]


@pytest.fixture(params=["py", "rs"])
def run(request: pytest.FixtureRequest, cli: Run, tmp_path: Path) -> Run:
    """Run the CLI in ``tmp_path`` with the backend forced by ``SQLTY_LIB``."""
    if request.param == "rs" and importlib.util.find_spec("sqlty._core") is None:
        pytest.skip("Rust backend not compiled")
    env = {**os.environ, "SQLTY_LIB": request.param}
    (tmp_path / "schema.sql").write_text(SCHEMA)

    def _run(*args: str) -> subprocess.CompletedProcess[str]:
        return cli(list(args), {"cwd": tmp_path, "env": env})

    return _run


def test_check_without_fix_leaves_file(run: Run, tmp_path: Path):
    q = tmp_path / "q.py"
    q.write_text(STALE)
    result = run("check", "--schema", "schema.sql", "q.py")
    assert result.returncode == 0
    assert result.stdout == (
        "q.py:2: [stale-type] SQL[tuple[str, str]] should be SQL[tuple[int, str]]\n"
        "q.py:3: [select-star] SELECT * used — expand to explicit column list\n"
        "q.py:4: [stale-rewrite] Source has --sql strings that aren't wrapped in SQL[T]\n"
        "Found 3 warning(s)\n"
    )
    assert q.read_text() == STALE


def test_check_fix_rewrites_file(run: Run, tmp_path: Path):
    q = tmp_path / "q.py"
    q.write_text(STALE)
    result = run("check", "--schema", "schema.sql", "--fix", "q.py")
    assert (result.returncode, result.stdout) == (0, "Fixed 1 file(s)\n")
    assert q.read_text() == FIXED

    again = run("check", "--schema", "schema.sql", "--fix", "-v", "q.py")
    assert (again.returncode, again.stdout) == (0, "")
    assert q.read_text() == FIXED


def test_check_fix_reports_only_remaining(run: Run, tmp_path: Path):
    q = tmp_path / "q.py"
    q.write_text(STALE + SKIPPED + 'd = SQL[tuple[int]]("--sql\\nSELECT * FROM orders")\n')
    result = run("check", "--fix", "-v", "q.py")
    assert result.returncode == 1
    assert result.stdout == (
        "Fixed q.py\n"
        "q.py:3: [select-star-no-schema] SELECT * used but no schema available\n"
        f"q.py:6: [fstring-skipped] {FSTRING}\n"
        f"q.py:7: [tstring-skipped] {TSTRING}\n"
        "q.py:8: [select-star-no-schema] SELECT * used but no schema available\n"
        "Fixed 1 file(s)\n"
        "Found 2 error(s), 2 warning(s)\n"
    )


def test_check_fix_respects_ignored_rules(run: Run, tmp_path: Path):
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nschema = "schema.sql"\n\n[tool.sqlty.rules]\nselect-star = "ignore"\n'
    )
    q = tmp_path / "q.py"
    q.write_text(STALE)
    result = run("check", "--fix", ".")
    assert result.returncode == 0
    assert 'b = SQL[tuple[int, str]]("--sql\\nSELECT * FROM users")' in q.read_text()


def test_check_reports_skipped_strings_not_stale_rewrite(run: Run, tmp_path: Path):
    (tmp_path / "q.py").write_text(SKIPPED)
    result = run("check", "q.py")
    assert result.returncode == 0
    assert result.stdout == (
        f"q.py:2: [fstring-skipped] {FSTRING}\n"
        f"q.py:3: [tstring-skipped] {TSTRING}\n"
        "Found 2 warning(s)\n"
    )


def test_skipped_string_severity_from_config(run: Run, tmp_path: Path):
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty.rules]\nfstring-skipped = "error"\ntstring-skipped = "ignore"\n'
    )
    (tmp_path / "q.py").write_text(SKIPPED)
    result = run("check", ".")
    assert result.returncode == 1
    assert result.stdout == f"q.py:2: [fstring-skipped] {FSTRING}\nFound 1 error(s)\n"

    rewrite = run("rewrite", ".")
    assert (rewrite.returncode, rewrite.stderr) == (0, f"q.py:2: [fstring-skipped] {FSTRING}\n")


def test_rewrite_prints_skipped_strings(run: Run, tmp_path: Path):
    q = tmp_path / "q.py"
    q.write_text(SKIPPED)
    result = run("rewrite", "--check", "q.py")
    assert result.returncode == 0
    assert result.stdout == "All files up to date\n"
    assert result.stderr == (
        f"q.py:2: [fstring-skipped] {FSTRING}\nq.py:3: [tstring-skipped] {TSTRING}\n"
    )
    assert q.read_text() == SKIPPED


def test_rewrite_quiet_hides_skipped_strings(run: Run, tmp_path: Path):
    (tmp_path / "q.py").write_text(SKIPPED)
    result = run("rewrite", "-q", "q.py")
    assert (result.returncode, result.stdout, result.stderr) == (0, "", "")

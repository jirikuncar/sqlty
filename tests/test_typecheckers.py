"""Tests that mypy, pyright, and ty all accept SQL[T] usage patterns.

Each test runs a type checker on a file in tests/typechecks/ and asserts
zero errors. The reveal_type() calls verify the inferred types match.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

import pytest

TYPECHECKS_DIR = Path(__file__).parent / "typechecks"
TYPECHECK_FILES = sorted(TYPECHECKS_DIR.glob("sql_*.py"))


def _ids() -> list[str]:
    return [f.stem for f in TYPECHECK_FILES]


# --- mypy ---


@pytest.mark.parametrize("path", TYPECHECK_FILES, ids=_ids())
def test_mypy(path: Path) -> None:
    """mypy reports no errors on SQL[T] usage."""
    if not shutil.which("mypy"):
        pytest.skip("mypy not installed")
    result = subprocess.run(
        ["mypy", "--no-error-summary", str(path)],
        capture_output=True,
        text=True,
    )
    # Filter out "note:" lines (reveal_type output)
    errors = [line for line in result.stdout.splitlines() if ": error:" in line]
    assert not errors, "mypy errors:\n" + "\n".join(errors)


# --- pyright ---


@pytest.mark.parametrize("path", TYPECHECK_FILES, ids=_ids())
def test_pyright(path: Path) -> None:
    """pyright reports no errors on SQL[T] usage."""
    if not shutil.which("pyright"):
        pytest.skip("pyright not installed")
    result = subprocess.run(
        ["pyright", str(path)],
        capture_output=True,
        text=True,
    )
    assert "0 errors" in result.stdout, f"pyright output:\n{result.stdout}"


# --- ty ---


@pytest.mark.parametrize("path", TYPECHECK_FILES, ids=_ids())
def test_ty(path: Path) -> None:
    """ty reports no errors on SQL[T] usage."""
    if not shutil.which("ty"):
        pytest.skip("ty not installed")
    result = subprocess.run(
        ["ty", "check", "--project", str(Path(__file__).parent.parent), str(path)],
        capture_output=True,
        text=True,
    )
    # ty uses "error" prefix for errors; "warning" and "info" are fine
    errors = [line for line in result.stdout.splitlines() if line.startswith("error")]
    assert not errors, "ty errors:\n" + "\n".join(errors)


# --- SQL[UNTYPED] is not Any ---

MISUSE = TYPECHECKS_DIR / "untyped_misuse.py"


@pytest.mark.parametrize(
    ("checker", "command", "marker"),
    [
        ("mypy", ["mypy", "--no-error-summary"], ": error:"),
        ("pyright", ["pyright"], "- error:"),
        ("ty", ["ty", "check", "--project", str(TYPECHECKS_DIR.parent.parent)], "error["),
    ],
    ids=["mypy", "pyright", "ty"],
)
def test_untyped_row_rejected(checker: str, command: list[str], marker: str) -> None:
    """Every checker rejects an SQL[UNTYPED] row used as tuple[int]."""
    if not shutil.which(checker):
        pytest.skip(f"{checker} not installed")
    result = subprocess.run([*command, str(MISUSE)], capture_output=True, text=True)
    errors = [line for line in result.stdout.splitlines() if marker in line]
    assert len(errors) == 1 and "UNTYPED" in errors[0], result.stdout


# --- a generated stub falls back to SQL[UNTYPED], not str ---

STUB_MISUSE = """from sqlty import SQL
from queries import sql


def rows[T](query: SQL[T]) -> list[T]:
    return []


typed: tuple[int] = rows(sql("SELECT id::INTEGER FROM users"))[0]
missing: tuple[int] = rows(sql("SELECT id::INTEGER FROM accounts"))[0]
"""


@pytest.mark.parametrize(
    ("checker", "command", "marker"),
    [
        ("mypy", ["mypy", "--no-error-summary"], ": error:"),
        ("pyright", ["pyright", "--pythonpath", sys.executable], "- error:"),
        ("ty", ["ty", "check", "--python", sys.prefix], "error["),
    ],
    ids=["mypy", "pyright", "ty"],
)
def test_untyped_stub_fallback_rejected(
    tmp_path: Path, checker: str, command: list[str], marker: str
) -> None:
    """A query missing from a sentinel-mode stub is SQL[UNTYPED]; its row is not tuple[int]."""
    if not shutil.which(checker):
        pytest.skip(f"{checker} not installed")
    from sqlty._generator import TypeStubGenerator

    (tmp_path / "queries.py").write_text(
        "from sqlty import sqlty\n\n\n@sqlty\ndef sql(query: str) -> str:\n    return query\n"
    )
    (tmp_path / "app.py").write_text(STUB_MISUSE.replace("accounts", "users"))
    TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query"}],
        untyped="sentinel",
    ).generate_stub_file(tmp_path, mode="incremental,create")
    (tmp_path / "app.py").write_text(STUB_MISUSE)

    result = subprocess.run(
        [*command, "app.py", "queries.pyi"], capture_output=True, text=True, cwd=tmp_path
    )
    errors = [line for line in result.stdout.splitlines() if marker in line]
    assert len(errors) == 1 and "UNTYPED" in errors[0], result.stdout

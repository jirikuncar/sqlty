"""Tests that mypy, pyright, and ty all accept SQL[T] usage patterns.

Each test runs a type checker on a file in tests/typechecks/ and asserts
zero errors. The reveal_type() calls verify the inferred types match.
"""

from __future__ import annotations

import os
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


# --- a sentinel-mode stub suppresses only the overlaps it creates ---

OVERLAP_CALLS = """from queries import sql

users = sql("SELECT id::INTEGER FROM users")
names = sql("SELECT 'a'::TEXT AS y")
pairs = sql("VALUES (1, 2)")
purge = sql("DELETE FROM users")
"""

UNRELATED_OVERLAP = """

@overload
def other(x: int) -> int: ...
@overload
def other(x: object) -> str: ...
"""


@pytest.mark.parametrize(
    ("checker", "command", "marker", "expected"),
    [
        ("mypy", ["mypy", "--no-error-summary", "--warn-unused-ignores"], ": error:", 1),
        ("pyright", ["pyright", "--pythonpath", sys.executable], "- error:", 1),
        # ty does not check overload overlap
        (
            "ty",
            ["ty", "check", "--python", sys.prefix, "--error", "unused-ignore-comment"],
            "error[",
            0,
        ),
    ],
    ids=["mypy", "pyright", "ty"],
)
def test_untyped_stub_overlap_scoped(
    tmp_path: Path, checker: str, command: list[str], marker: str, expected: int
) -> None:
    """Generated overloads need no unused ignores; an unrelated overlap is still reported.

    pyright is run without ``reportUnnecessaryTypeIgnoreComment``: it reports only the first
    incompatible overload per catch-all, so it would flag the ignores mypy needs on later ones.
    """
    if not shutil.which(checker):
        pytest.skip(f"{checker} not installed")
    from sqlty._generator import TypeStubGenerator

    (tmp_path / "queries.py").write_text(
        "from sqlty import sqlty\n\n\n@sqlty\ndef sql(query: str) -> str:\n    return query\n"
    )
    (tmp_path / "app.py").write_text(OVERLAP_CALLS)
    stub = TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query"}],
        untyped="sentinel",
    ).generate_stub_file(tmp_path, mode="incremental,create")
    assert stub.read_text().count("# type: ignore[overload-overlap]") == 2
    stub.write_text(stub.read_text() + UNRELATED_OVERLAP)
    other = stub.read_text().splitlines().index("def other(x: int) -> int: ...") + 1

    result = subprocess.run(
        [*command, "app.py", "queries.pyi"], capture_output=True, text=True, cwd=tmp_path
    )
    errors = [line for line in result.stdout.splitlines() if marker in line]
    assert len(errors) == expected, result.stdout
    assert all(f"queries.pyi:{other}:" in error for error in errors), result.stdout


# --- a runtime stub for a helper with a defaulted query parameter ---

DEFAULT_HELPER = """from sqlty import sqlty


@sqlty
def kw(conn: object, q: str = "SELECT 1::INTEGER") -> object:
    return q


@sqlty
def ko(conn: object, *, q: str = "SELECT 1::INTEGER") -> object:
    return q
"""

DEFAULT_CALLS = """from sqlty import SQL
from queries import ko, kw

a: SQL[tuple[int]] = kw("c")
b: SQL[tuple[str]] = kw("c", "SELECT 'a'::TEXT")
c: SQL[tuple[int]] = ko("c")
d: SQL[tuple[str]] = ko("c", q="SELECT 'a'::TEXT")
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
def test_runtime_stub_query_default(
    tmp_path: Path, checker: str, command: list[str], marker: str
) -> None:
    """Literal overloads of a defaulted query parameter are valid and resolve omitted queries."""
    if not shutil.which(checker):
        pytest.skip(f"{checker} not installed")
    (tmp_path / "pyproject.toml").write_text(
        "".join(
            f'[[tool.sqlty.functions]]\nimport_path = "queries.{name}"\nquery_param = "q"\n\n'
            for name in ["kw", "ko"]
        )
    )
    (tmp_path / "queries.py").write_text(DEFAULT_HELPER)
    (tmp_path / "app.py").write_text(DEFAULT_CALLS)
    generated = subprocess.run(
        [sys.executable, "app.py"],
        capture_output=True,
        text=True,
        cwd=tmp_path,
        env={**os.environ, "SQLTY_AUTO_GENERATE": "1"},
    )
    assert generated.returncode == 0, generated.stderr

    result = subprocess.run(
        [*command, "app.py", "queries.pyi"], capture_output=True, text=True, cwd=tmp_path
    )
    errors = [line for line in result.stdout.splitlines() if marker in line]
    assert not errors, result.stdout + (tmp_path / "queries.pyi").read_text()

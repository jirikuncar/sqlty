"""Example template for writing new e2e tests.

This file serves as a template and quick reference for writing new end-to-end tests.
Copy and modify this template when adding new test cases.

Key principles:
1. Use subprocess to run sqlty CLI (no internal imports)
2. Use inline_snapshot for result verification
3. Include mypy validation when testing type checking
4. Test complete workflows from source to validation
"""

from collections.abc import Callable
from pathlib import Path
from subprocess import CompletedProcess

from inline_snapshot import snapshot


def test_example_basic_pattern(
    tmp_path: Path,
    cli: Callable[..., CompletedProcess[str]],
    mypy: Callable[..., CompletedProcess[str]],
) -> None:
    """Example: Basic test pattern with snapshot."""
    # Step 1: Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import sqlty


@sqlty
def sql(query: str):
    """Your SQL query function."""
    return query
''')

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Your SQL queries here
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
""")

    # Step 2: Run sqlty generator via subprocess
    result = cli([str(tmp_path)])

    # Verify generator succeeded
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Step 3: Verify stub file was created
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    # Step 4: Snapshot the generated content
    stub_content = stub_file.read_text()
    assert stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL


@overload
def sql(query: Literal['SELECT id::INTEGER, name::TEXT FROM users']) -> SQL[tuple[int, str]]:
    ...

@overload
def sql(query: str) -> str:
    ...
""")

    # Step 5 (Optional): Run mypy to validate type checking
    mypy_result = mypy([str(app_file), "--strict"])

    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"


def test_example_without_mypy(tmp_path: Path, cli: Callable[..., CompletedProcess[str]]) -> None:
    """Example: Test without mypy validation (for CLI behavior tests)."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import sqlty


@sqlty
def sql(query: str):
    return query
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT 1::INTEGER")
""")

    # Test CLI behavior without mypy
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    # Verify file exists
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    # Check specific content without full snapshot
    stub_content = stub_file.read_text()
    assert "SELECT 1::INTEGER" in stub_content


def test_example_error_case(tmp_path: Path, cli: Callable[..., CompletedProcess[str]]) -> None:
    """Example: Testing error handling."""
    # Test with invalid input
    nonexistent = tmp_path / "nonexistent"

    result = cli([str(nonexistent)])

    # Expect non-zero exit code
    assert result.returncode != 0 or "not found" in result.stderr.lower()


def test_example_cli_options(tmp_path: Path, cli: Callable[..., CompletedProcess[str]]) -> None:
    """Example: Testing CLI options."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import sqlty


@sqlty
def sql(query: str):
    return query
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Test with specific CLI option
    result = cli([str(tmp_path), "--dialect=postgres"])

    assert result.returncode in (0, 2)  # 2 if option not recognized


def test_example_incremental_behavior(
    tmp_path: Path, cli: Callable[..., CompletedProcess[str]]
) -> None:
    """Example: Testing incremental updates."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import sqlty


@sqlty
def sql(query: str):
    return query
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT id::INTEGER FROM users")
""")

    # Initial generation
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    initial_stub = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in initial_stub

    # Add new query
    app_file.write_text("""from queries import sql

query1 = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
""")

    # Regenerate
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Verify both queries are present
    updated_stub = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in updated_stub
    assert "SELECT name::TEXT FROM users" in updated_stub


# Note: See tests/e2e/README.md for tips on writing new tests

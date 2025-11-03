"""Example template for writing new e2e tests.

This file serves as a template and quick reference for writing new end-to-end tests.
Copy and modify this template when adding new test cases.

Key principles:
1. Use subprocess to run sqlty CLI (no internal imports)
2. Use inline_snapshot for result verification
3. Include mypy validation when testing type checking
4. Test complete workflows from source to validation
"""

import subprocess
import sys
from pathlib import Path

from inline_snapshot import snapshot


def test_example_basic_pattern(tmp_path: Path) -> None:
    """Example: Basic test pattern with snapshot."""
    # Step 1: Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    """Your query registry."""

    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

# Your SQL queries here
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
''')

    # Step 2: Run sqlty generator via subprocess
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    # Verify generator succeeded
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Step 3: Verify stub file was created
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    # Step 4: Snapshot the generated content
    stub_content = stub_file.read_text()
    assert stub_content == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    \"\"\"Your query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
    ) -> SQL[tuple[int, str]]:
        # from app:4
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = Query.sql
""")

    # Step 5 (Optional): Run mypy to validate type checking
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"


def test_example_without_mypy(tmp_path: Path) -> None:
    """Example: Test without mypy validation (for CLI behavior tests)."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT 1::INTEGER")
''')

    # Test CLI behavior without mypy
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Verify file exists
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    # Check specific content without full snapshot
    stub_content = stub_file.read_text()
    assert "SELECT 1::INTEGER" in stub_content


def test_example_error_case(tmp_path: Path) -> None:
    """Example: Testing error handling."""
    # Test with invalid input
    nonexistent = tmp_path / "nonexistent"

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(nonexistent)],
        capture_output=True,
        text=True,
    )

    # Expect non-zero exit code
    assert result.returncode != 0 or "not found" in result.stderr.lower()


def test_example_cli_options(tmp_path: Path) -> None:
    """Example: Testing CLI options."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    # Test with specific CLI option
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--dialect=postgres"],
        capture_output=True,
        text=True,
    )

    assert result.returncode in (0, 2)  # 2 if option not recognized


def test_example_incremental_behavior(tmp_path: Path) -> None:
    """Example: Testing incremental updates."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query1 = sql("SELECT id::INTEGER FROM users")
''')

    # Initial generation
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    initial_stub = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in initial_stub

    # Add new query
    app_file.write_text('''from queries import sql

query1 = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
''')

    # Regenerate
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0

    # Verify both queries are present
    updated_stub = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in updated_stub
    assert "SELECT name::TEXT FROM users" in updated_stub


# Tips for writing tests:
# 1. Use descriptive test names that explain what is being tested
# 2. Add docstrings explaining the test purpose
# 3. Use tmp_path for all file operations
# 4. Assert success before checking content
# 5. Use snapshot() for first run, then update with --inline-snapshot=update
# 6. Include mypy validation for type-safety tests
# 7. Test both success and failure scenarios
# 8. Keep tests focused on one aspect at a time

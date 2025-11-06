"""End-to-end tests for the @sqlty decorator pattern.

Test the decorator workflow:
1. Define functions decorated with @sqlty
2. Run stub generator via subprocess
3. Validate generated stubs
4. Run mypy to ensure type safety
"""

import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any


def test_simple_decorator_pattern(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    mypy: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    stubtest: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test basic @sqlty decorator without schema."""
    # Create app.py with @sqlty decorator
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using @sqlty decorator."""

from typing import TYPE_CHECKING, Any

from sqlty import sqlty

if TYPE_CHECKING:
    from sqlty import SQL



@sqlty
def sql(query: str) -> Any:
    return query


q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
q2: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")
''')

    # Generate stubs
    result = cli([str(tmp_path), "--mode=full"], {"cwd": str(tmp_path)})
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated
    stub_file = tmp_path / "app.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    # Verify stub content contains expected elements
    assert "from typing import Literal, overload" in stub_content
    assert "@overload" in stub_content
    assert "SELECT 1::INTEGER" in stub_content
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content
    assert "SQL[tuple[int]]" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content

    # Run mypy to validate type safety
    mypy_result = mypy([str(app_file), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"

    # Verify with stubtest
    stubtest_result = stubtest(
        ["app", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_decorator_with_schema(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    mypy: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    stubtest: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test @sqlty decorator with schema parameter."""
    # Create schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text("""
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
""")

    # Create app.py with @sqlty(schema="schema.sql")
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using @sqlty decorator with schema."""

from typing import TYPE_CHECKING, Any

from sqlty import sqlty

if TYPE_CHECKING:
    from sqlty import SQL

@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    return query

q: SQL[tuple[int, str]] = sql_with_schema("SELECT id, name FROM users")
''')

    # Generate stubs with schema
    result = cli([str(tmp_path), "--mode=full"], {"cwd": str(tmp_path)})
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated
    stub_file = tmp_path / "app.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    # Verify stub content
    assert "from typing import Literal, overload" in stub_content
    assert "@overload" in stub_content
    assert "SELECT id, name FROM users" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content

    # Run mypy to validate type safety
    mypy_result = mypy([str(app_file), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"

    # Verify with stubtest
    stubtest_result = stubtest(
        ["app", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_multiple_decorated_functions(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    mypy: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    stubtest: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test multiple functions with @sqlty decorator."""
    # Create app.py with multiple decorated functions
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application with multiple @sqlty decorated functions."""

from typing import TYPE_CHECKING, Any

from sqlty import sqlty

if TYPE_CHECKING:
    from sqlty import SQL

@sqlty
def sql(query: str) -> Any:
    return query

@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    return query

q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
q2: SQL[tuple[int, str]] = sql_with_schema("SELECT id::INTEGER, name::TEXT FROM users")
''')

    # Generate stubs
    stub_file = tmp_path / "app.pyi"
    result = cli([str(tmp_path), "--mode=full", f"--output={stub_file}"], {"cwd": str(tmp_path)})
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    # Verify both functions are in the stub
    assert "@overload" in stub_content
    assert "def sql(" in stub_content
    assert "def sql_with_schema(" in stub_content
    assert "SELECT 1::INTEGER" in stub_content
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content

    # Run mypy to validate type safety
    mypy_result = mypy([str(app_file), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"

    # Verify with stubtest
    stubtest_result = stubtest(
        ["app", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_decorator_function_attributes(
    tmp_path: Path,
) -> None:
    """Test that decorated functions have __sqlty__ and __schema_path__ attributes."""
    # Create a test script that checks attributes at runtime
    test_script = tmp_path / "test_attrs.py"
    test_script.write_text("""
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

@sqlty(schema="schema.sql")
def sql_with_schema[T](query: T) -> T:
    return query

# Test __sqlty__ attribute
assert hasattr(sql, "__sqlty__"), "sql should have __sqlty__ attribute"
assert sql.__sqlty__ is True, "sql.__sqlty__ should be True"

assert hasattr(sql_with_schema, "__sqlty__"), "sql_with_schema should have __sqlty__ attribute"
assert sql_with_schema.__sqlty__ is True, "sql_with_schema.__sqlty__ should be True"

# Test __schema_path__ attribute
assert not hasattr(sql, "__schema_path__"), "sql should not have __schema_path__ attribute"

assert hasattr(sql_with_schema, "__schema_path__"), "sql_with_schema should have __schema_path__ attribute"
assert sql_with_schema.__schema_path__ == "schema.sql", f"Expected 'schema.sql', got {sql_with_schema.__schema_path__}"

print("All attribute tests passed!")
""")

    # Run the test script
    result = subprocess.run(
        [sys.executable, str(test_script)],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0, f"Attribute test failed: {result.stderr}\n{result.stdout}"
    assert "All attribute tests passed!" in result.stdout

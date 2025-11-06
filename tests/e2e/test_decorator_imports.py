"""End-to-end tests for @sqlty decorator pattern with imports.

Test that decorated functions work when imported from other modules.
"""

import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any


def test_decorated_function_imported_from_another_module(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    mypy: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    stubtest: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test that decorated functions work when imported using relative imports."""
    # Create package structure
    pkg_dir = tmp_path / "mypackage"
    pkg_dir.mkdir()

    # Create __init__.py
    (pkg_dir / "__init__.py").write_text("")

    # Create sql.py with decorated function
    sql_file = pkg_dir / "sql.py"
    sql_file.write_text('''"""SQL query function."""

from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    return query
''')

    # Create app.py that imports from sql
    app_file = pkg_dir / "app.py"
    app_file.write_text('''"""Application using imported SQL function."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from sqlty import SQL

from .sql import sql


q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
q2: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")
''')

    # Generate stubs
    result = cli([str(pkg_dir)], {"cwd": str(tmp_path)})
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated for sql.py
    sql_stub = pkg_dir / "sql.pyi"
    assert sql_stub.exists(), "sql.pyi was not generated"

    stub_content = sql_stub.read_text()

    # Verify stub contains both queries (from sql.py definition and app.py usage)
    assert "@overload" in stub_content
    assert "def sql(" in stub_content
    assert "SELECT 1::INTEGER" in stub_content
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content
    assert "SQL[tuple[int]]" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content

    # Run mypy to validate type safety
    mypy_result = mypy([str(app_file), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"

    # Verify with stubtest
    stubtest_result = stubtest(
        ["mypackage.sql", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_decorated_function_imported_with_absolute_import(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    mypy: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test that decorated functions work with absolute imports (from same directory)."""
    # Create sql.py with decorated function
    sql_file = tmp_path / "sql.py"
    sql_file.write_text('''"""SQL query function."""

from sqlty import sqlty


@sqlty
def sql[T](query: T) -> T:
    return query
''')

    # Create app.py that imports from sql (same directory)
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using imported SQL function."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from sqlty import SQL

from sql import sql


q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
q2: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")
''')

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated for sql.py
    sql_stub = tmp_path / "sql.pyi"
    assert sql_stub.exists(), "sql.pyi was not generated"

    stub_content = sql_stub.read_text()

    # Verify stub contains both queries
    assert "@overload" in stub_content
    assert "def sql(" in stub_content
    assert "SELECT 1::INTEGER" in stub_content
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content

    # Run mypy to validate type safety
    mypy_result = mypy([str(app_file), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"


def test_decorated_function_runtime_attributes_after_import(
    tmp_path: Path,
) -> None:
    """Test that decorated functions maintain their attributes after being imported."""
    # Create package structure
    pkg_dir = tmp_path / "mypackage"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")

    # Create sql.py with decorated function
    (pkg_dir / "sql.py").write_text("""from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    return query
""")

    # Create test script that imports and checks attributes
    test_script = tmp_path / "test_attrs.py"
    test_script.write_text(f"""import sys
sys.path.insert(0, {str(tmp_path)!r})

from mypackage.sql import sql, sql_with_schema

# Test __sqlty__ attribute
assert hasattr(sql, "__sqlty__"), "sql should have __sqlty__ attribute"
assert sql.__sqlty__ is True, "sql.__sqlty__ should be True"

assert hasattr(sql_with_schema, "__sqlty__"), "sql_with_schema should have __sqlty__ attribute"
assert sql_with_schema.__sqlty__ is True, "sql_with_schema.__sqlty__ should be True"

# Test __schema_path__ attribute
assert not hasattr(sql, "__schema_path__"), "sql should not have __schema_path__ attribute"

assert hasattr(sql_with_schema, "__schema_path__"), "sql_with_schema should have __schema_path__ attribute"
assert sql_with_schema.__schema_path__ == "schema.sql", f"Expected 'schema.sql', got {{sql_with_schema.__schema_path__}}"

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

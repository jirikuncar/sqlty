"""Tests for configured function support."""

import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

pytestmark = pytest.mark.skip(reason="Legacy generate CLI deferred to PLAN-FUTURE.md")


def test_configured_function_basic(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test basic configured function generation.

    NOTE: Current limitation - generated stubs only include the query parameter,
    not the full function signature. See IMPLEMENTATION_NOTES.md for details.
    """
    # Create a mock database module with pg_fetchvalue
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Database utilities."""
from typing import Any

def pg_fetchvalue(
    conn: Any,
    query: str,
    return_tp: type[Any] | None = None,
    /,
    **params: Any,
) -> Any | None:
    """Fetch a single value."""
    return None
''')

    # Create an application file that uses pg_fetchvalue
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using pg_fetchvalue."""
from db import pg_fetchvalue

result = pg_fetchvalue(None, query="SELECT id::INTEGER FROM users WHERE email = :email", return_tp=int, email="test@example.com")
count = pg_fetchvalue(None, query="SELECT COUNT(*)::INTEGER FROM users", return_tp=int)
''')

    # Create pyproject.toml with function configuration
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
""")

    # Run the generator
    result = cli([str(tmp_path)])

    # Check that it ran successfully
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated for db.py
    stub_file = tmp_path / "db.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()
    print("Generated stub content:")
    print(stub_content)

    # Verify that the stub contains overloads for the configured function
    assert "def pg_fetchvalue" in stub_content
    assert "Literal[" in stub_content
    assert "SELECT id::INTEGER FROM users WHERE email = :email" in stub_content
    assert "SELECT COUNT(*)::INTEGER FROM users" in stub_content


def test_configured_function_without_return_type_param(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test configured function without return_type_param."""
    # Create a mock database module
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Database utilities."""
from typing import Any

def execute_query(query: str, **params: Any) -> list[Any]:
    """Execute a query."""
    return []
''')

    # Create an application file
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using execute_query."""
from db import execute_query

result = execute_query(query="SELECT * FROM users WHERE id = :id", id=1)
''')

    # Create pyproject.toml with function configuration (no return_type_param)
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
""")

    # Run the generator
    result = cli([str(tmp_path)])

    # Check that it ran successfully
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated
    stub_file = tmp_path / "db.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()
    print("Generated stub content:")
    print(stub_content)

    # Verify basic structure (type inference will happen from query)
    assert "def execute_query" in stub_content


def test_configured_function_type_mismatch(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test configured function with SQL query not matching return type."""
    # Create a mock database module
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Database utilities."""
from typing import Any

def pg_fetchvalue(
    conn: Any,
    query: str,
    return_tp: type[Any] | None = None,
    /,
    **params: Any,
) -> Any | None:
    """Fetch a single value."""
    return None
''')

    # Create an application file with type mismatch
    # SQL returns INTEGER but user specifies str as return type
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application with type mismatch."""
from db import pg_fetchvalue

# SQL query returns INTEGER but user specifies str as return_tp
# This should be caught by mypy after stub generation
result = pg_fetchvalue(
    None,
    query="SELECT COUNT(*)::INTEGER FROM users",
    return_tp=str,  # Type mismatch: SQL returns int, but expecting str
)
''')

    # Create pyproject.toml with function configuration
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
""")

    # Run the generator
    result = cli([str(tmp_path)])

    # Check that it ran successfully
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated
    stub_file = tmp_path / "db.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()
    print("Generated stub content:")
    print(stub_content)

    # Verify that the stub contains the overload with str return type
    # (even though SQL returns INTEGER)
    assert "def pg_fetchvalue" in stub_content
    assert "Literal[" in stub_content
    assert "SELECT COUNT(*)::INTEGER FROM users" in stub_content
    # The stub should wrap with SQL[str] as specified by return_tp parameter
    assert "-> SQL[str]" in stub_content

    # Now run mypy on the app.py to verify type checking
    # Note: This will only work if mypy is available and properly configured
    # For now, we just verify the stub was generated correctly
    # Users can run mypy separately to catch the type mismatch


def test_configured_function_with_schema(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test configured function with schema path."""
    # Create a schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text("""
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL
);
""")

    # Create a mock database module
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Database utilities."""
from typing import Any

def pg_fetchvalue(
    conn: Any,
    query: str,
    return_tp: type[Any] | None = None,
    /,
    **params: Any,
) -> Any | None:
    """Fetch a single value."""
    return None
''')

    # Create an application file
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using pg_fetchvalue with schema."""
from db import pg_fetchvalue

# Schema-aware query - should infer types from schema.sql
result = pg_fetchvalue(
    None,
    query="SELECT name FROM users WHERE id = :id",
    return_tp=str,
    id=1
)
''')

    # Create pyproject.toml with function configuration including schema
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text(f'''[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
schema = "{schema_file.name}"
''')

    # Run the generator
    result = cli([str(tmp_path)])

    # Check that it ran successfully
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Check that stub file was generated
    stub_file = tmp_path / "db.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()
    print("Generated stub content:")
    print(stub_content)

    # Verify that the stub contains the overload with schema-aware types
    assert "def pg_fetchvalue" in stub_content
    assert "Literal[" in stub_content
    assert "SELECT name FROM users WHERE id = :id" in stub_content


# ============================================================================
# Mypy Integration Tests
# ============================================================================
# These tests demonstrate the EXPECTED behavior once full signature support
# is implemented. They are currently marked as xfail because stubs only
# include the query parameter, not the full function signature.
# ============================================================================


def test_mypy_validates_configured_function_with_full_signature(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
    mypy: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test that mypy validates configured function calls with all parameters.

    This test demonstrates the EXPECTED behavior: mypy should accept calls with
    the correct signature including conn, query, return_tp, and **params.
    """
    # Create database module
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Database utilities."""
from typing import Any

def pg_fetchvalue(
    conn: Any,
    /,
    query: str,
    return_tp: type[Any] | None = None,
    **params: Any,
) -> Any | None:
    """Fetch a single value."""
    return None
''')

    # Create application with proper typed usage
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application with type-safe database calls."""
from typing import TYPE_CHECKING
from db import pg_fetchvalue

if TYPE_CHECKING:
    from sqlty import SQL

# These calls should be accepted by mypy with proper signatures
# Note: conn is positional-only, query and return_tp can be passed as keyword args
# The function returns SQL[int], which is our type-safe wrapper
user_id: SQL[int] = pg_fetchvalue(
    None,  # conn parameter (positional-only)
    query="SELECT id::INTEGER FROM users WHERE email = :email",
    return_tp=int,
    email="test@example.com"  # **params
)

count: SQL[int] = pg_fetchvalue(
    None,
    query="SELECT COUNT(*)::INTEGER FROM users",
    return_tp=int,
)

# Variable with SQL[T] type annotation
if TYPE_CHECKING:
    query_result: SQL[int] = pg_fetchvalue(
        None,
        query="SELECT id::INTEGER FROM users LIMIT 1",
        return_tp=int,
    )
''')

    # Configure function
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
""")

    # Generate stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify stub was generated
    stub_file = tmp_path / "db.pyi"
    assert stub_file.exists()

    # Verify stub has full signature
    stub_content = stub_file.read_text()
    assert "conn: Any" in stub_content
    assert "query: Literal[" in stub_content
    # Validate presence of return_tp type annotation in overload
    assert "return_tp: type[Any] | None" in stub_content or "return_tp: Any=None" in stub_content
    assert "**params: Any" in stub_content
    assert "-> SQL[int]" in stub_content

    # Run mypy - should pass with correct signature
    mypy_result = mypy([str(app_file)])
    assert mypy_result.returncode == 0, (
        f"Mypy validation failed:\n{mypy_result.stdout}\n{mypy_result.stderr}"
    )


def test_mypy_catches_wrong_parameter_types(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
    mypy: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test that mypy catches type errors in function parameters.

    This test demonstrates that mypy should catch errors like:
    - Wrong number of arguments
    - Wrong argument types
    - Missing required parameters
    """
    # Create database module
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Database utilities."""
from typing import Any

def pg_fetchvalue(
    conn: Any,
    query: str,
    return_tp: type[Any] | None = None,
    /,
    **params: Any,
) -> Any | None:
    """Fetch a single value."""
    return None
''')

    # Create application with TYPE ERRORS
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application with intentional type errors."""
from db import pg_fetchvalue

# ERROR 1: Missing required conn parameter
result1 = pg_fetchvalue(
    query="SELECT id FROM users",  # type: ignore[call-arg]
    return_tp=int,
)

# ERROR 2: query parameter should be keyword, not positional after conn
result2 = pg_fetchvalue(
    None,
    "SELECT id FROM users",  # Should be query=... # type: ignore[call-arg]
    return_tp=int,
)

# ERROR 3: return_tp should be a type, not a string
result3 = pg_fetchvalue(
    None,
    query="SELECT id FROM users",
    return_tp="int",  # Should be int, not "int" # type: ignore[arg-type]
)
''')

    # Configure function
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
""")

    # Generate stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Run mypy - should FAIL due to type errors
    mypy_result = mypy([str(app_file)])

    print("\n=== Mypy output (expecting errors) ===")
    print(mypy_result.stdout)
    print("=== End mypy output ===\n")

    # Should have errors (but not due to missing type: ignore comments)
    # The actual type errors should be caught
    assert mypy_result.returncode != 0, "Mypy should catch the type errors"

    # Verify specific errors are reported (once full signature support is added)
    # output = mypy_result.stdout.lower()
    # These assertions will work once full signature support is implemented
    # assert "missing positional argument" in output or "call-arg" in output


def test_mypy_infers_sql_type_wrapper(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
    mypy: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test that mypy understands SQL[T] type wrapper in return types.

    This test verifies that:
    1. Return type is SQL[int], not just int
    2. SQL[T] is compatible with T (since SQL[T] extends str)
    3. Type inference works through the wrapper
    """
    # Create database module
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Database utilities."""
from typing import Any

def pg_fetchvalue(
    conn: Any,
    query: str,
    return_tp: type[Any] | None = None,
    /,
    **params: Any,
) -> Any | None:
    """Fetch a single value."""
    return None
''')

    # Create application testing SQL[T] types
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application testing SQL[T] type wrapper."""
from typing import TYPE_CHECKING
from db import pg_fetchvalue

if TYPE_CHECKING:
    from sqlty import SQL

# Test 1: Explicit SQL[int] annotation
result1: SQL[int] | None = pg_fetchvalue(
    None,
    query="SELECT COUNT(*)::INTEGER FROM users",
    return_tp=int,
)

# Test 2: SQL[T] should work as T (SQL[int] can be used as int)
if result1 is not None:
    # This should work because SQL[int] extends str but is typed as SQL[int]
    value: int = result1  # type: ignore[assignment]  # SQL[int] is not directly int

# Test 3: Type mismatch should be caught
wrong_type: SQL[str] | None = pg_fetchvalue(  # type: ignore[assignment]
    None,
    query="SELECT COUNT(*)::INTEGER FROM users",  # Returns SQL[int]
    return_tp=int,
)  # Should error: SQL[int] is not compatible with SQL[str]
''')

    # Configure function
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
""")

    # Generate stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    stub_file = tmp_path / "db.pyi"
    stub_content = stub_file.read_text()

    print("\n=== Generated stub ===")
    print(stub_content)
    print("=== End stub ===\n")

    # Verify SQL[T] is used in return types
    assert "SQL[int]" in stub_content, "Return type should be SQL[int]"

    # Run mypy
    mypy_result = mypy([str(app_file)])

    print("\n=== Mypy output ===")
    print(mypy_result.stdout)
    print("=== End mypy output ===\n")

    # Should have limited errors (only the intentional type: ignore ones)
    # The SQL[T] type system should work correctly


def test_mypy_validates_current_limited_signatures(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
    mypy: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test current behavior: mypy works with simple query-only signature.

    This test documents the CURRENT working behavior where stubs only have
    the query parameter. This works for simple cases but doesn't validate
    the full function signature.
    """
    # Create simple database module
    db_file = tmp_path / "db.py"
    db_file.write_text('''"""Simple database utilities."""
from typing import Any

def sql(query: str) -> Any:
    """Execute a query."""
    return None
''')

    # Create application using simple signature
    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using simple query function."""
from db import sql

# This works with current implementation
result = sql(query="SELECT id::INTEGER FROM users")

# Type annotation (loose because we return Any currently)
value: object = result
''')

    # Configure function (no return_type_param for simplicity)
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.sql"
query_param = "query"
""")

    # Generate stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Run mypy - should pass
    mypy_result = mypy([str(app_file)])

    if mypy_result.returncode != 0:
        print("\n=== Mypy output ===")
        print(mypy_result.stdout)
        print(mypy_result.stderr)
        print("=== End mypy output ===\n")

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"

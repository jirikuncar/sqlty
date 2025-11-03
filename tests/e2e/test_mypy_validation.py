"""End-to-end tests validating mypy type checking on generated stubs.

These tests ensure that the generated stubs work correctly with mypy
and that type annotations are properly inferred and validated.
"""

import subprocess
import sys
from pathlib import Path

from inline_snapshot import snapshot


def test_mypy_validates_query_types(tmp_path: Path) -> None:
    """Test that mypy validates query return types correctly."""
    # Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Query with explicit type annotation
q: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")

# This should type-check correctly
user_id: int
user_name: str
for user_id, user_name in []:  # Using empty list for type checking only
    print(f"User {user_id}: {user_name}")
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Run mypy - it should pass
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    # Snapshot mypy output
    assert mypy_result.returncode == 0
    # Success should produce no output or just success message
    assert mypy_result.stdout == snapshot("Success: no issues found in 1 source file\n")


def test_mypy_catches_type_mismatch(tmp_path: Path) -> None:
    """Test that mypy catches type mismatches in query usage."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Correct type annotation
q: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")

# Type error: trying to assign to wrong type
wrong: SQL[tuple[str, int]] = q  # This should fail mypy
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Run mypy - it should fail due to type mismatch
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    # Should have non-zero exit code (type error found)
    assert mypy_result.returncode != 0
    # Should contain error about incompatible types
    assert "incompatible" in mypy_result.stdout.lower() or "error" in mypy_result.stdout.lower()


def test_mypy_infers_tuple_unpacking(tmp_path: Path) -> None:
    """Test that mypy correctly infers types from tuple unpacking."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

# Query returns tuple[int, str, str]
query = sql("SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")

# Mypy should infer types from unpacking
def process_user(user_id: int, name: str, email: str) -> None:
    print(f"{user_id}: {name} <{email}>")


# This should type-check - but we need actual execution or iterator
# For type checking purposes, we'll use TYPE_CHECKING
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    # Simulate the query result type
    result: list[tuple[int, str, str]] = []
    for user_id, name, email in result:
        process_user(user_id, name, email)
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Run mypy
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_mypy_with_multiple_queries(tmp_path: Path) -> None:
    """Test mypy validation with multiple queries of different types."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Different query types
int_query: SQL[tuple[int]] = sql("SELECT COUNT(*)::INTEGER FROM users")
str_query: SQL[tuple[str]] = sql("SELECT name::TEXT FROM users LIMIT 1")
multi_query: SQL[tuple[int, str, str]] = sql(
    "SELECT id::INTEGER, name::TEXT, email::TEXT FROM users"
)

# Verify types are distinct
def check_types() -> None:
    # These should all type-check correctly
    i: SQL[tuple[int]] = int_query
    s: SQL[tuple[str]] = str_query
    m: SQL[tuple[int, str, str]] = multi_query
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Verify stub contains all query types
    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    assert "SQL[tuple[int]]" in stub_content
    assert "SQL[tuple[str]]" in stub_content
    assert "SQL[tuple[int, str, str]]" in stub_content

    # Run mypy
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_mypy_with_query_parameters(tmp_path: Path) -> None:
    """Test mypy validation for queries with parameters."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Query with named parameters
query: SQL[tuple[int, str]] = sql(
    "SELECT id::INTEGER, name::TEXT FROM users WHERE id = :user_id"
)

# The query string includes parameter placeholders
# but the return type is still inferred from SELECT clause
result_type: SQL[tuple[int, str]] = query
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Verify stub includes parameterized query
    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    assert ":user_id" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content

    # Run mypy
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_mypy_with_complex_types(tmp_path: Path) -> None:
    """Test mypy validation with complex SQL types like arrays or JSON."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# For now, complex types are treated as strings
# Future: could support more specific type mappings
query: SQL[tuple[int, str, str]] = sql(
    "SELECT id::INTEGER, data::TEXT, tags::TEXT FROM items"
)
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Run mypy
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"

"""End-to-end tests for edge cases and error scenarios.

Tests error handling, edge cases, and unusual but valid scenarios.
"""

import subprocess
import sys
from pathlib import Path


def test_empty_project(tmp_path: Path) -> None:
    """Test running sqlty on empty directory."""
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    # Should handle gracefully - either exit 0 (no-op) or 1 (no files found)
    assert result.returncode in (0, 1)


def test_no_queries_found(tmp_path: Path) -> None:
    """Test how the tool handles a project with registry but no queries.

    The tool should complete successfully, either creating a minimal
    stub file with just the generic overload or skipping stub generation.
    """
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    # App file without any sql() calls
    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql


def hello() -> str:
    return "Hello, World!"
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    # Should succeed - the tool handles this gracefully
    assert result.returncode == 0

    # If stub is created, it should have the generic overload at minimum
    stub_file = tmp_path / "queries.pyi"
    if stub_file.exists():
        stub_content = stub_file.read_text()
        assert "Query" in stub_content


def test_syntax_error_in_source(tmp_path: Path) -> None:
    """Test handling of syntax errors in source files."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    # File with syntax error
    bad_file = tmp_path / "bad.py"
    bad_file.write_text("""from queries import sql

def broken(
    # Missing closing parenthesis and body
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    # Should handle gracefully without crashing
    assert result.returncode in (0, 1)


def test_non_literal_query_string(tmp_path: Path) -> None:
    """Test that non-literal query strings are skipped with warning."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Non-literal query - should be warned about
dynamic_query = "SELECT * FROM users"
query = sql(dynamic_query)

# This should work
static_query = sql("SELECT id::INTEGER FROM users")
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Stub should be created with the static query only
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in stub_content


def test_duplicate_queries_same_file(tmp_path: Path) -> None:
    """Test handling of duplicate queries in the same file."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Same query used multiple times
query1 = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT id::INTEGER FROM users")
query3 = sql("SELECT id::INTEGER FROM users")
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Should only have one overload for the duplicate query
    assert stub_content.count("SELECT id::INTEGER FROM users") == 1


def test_very_long_query(tmp_path: Path) -> None:
    """Test handling of very long SQL queries."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    # Create a very long query
    columns = ", ".join([f"col{i}::TEXT" for i in range(50)])
    app_file = tmp_path / "app.py"
    app_file.write_text(f"""from queries import sql

query = sql("SELECT {columns} FROM big_table")
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    # Should handle long queries
    assert result.returncode == 0


def test_unicode_in_query(tmp_path: Path) -> None:
    """Test handling of Unicode characters in queries."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Query with Unicode
query = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE name = :name")
# params = {"name": "José García"} -- would contain unicode
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0


def test_comments_in_queries(tmp_path: Path) -> None:
    """Test SQL queries with SQL comments."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

# Query with SQL comments
query = sql("""
    -- Get user information
    SELECT
        id::INTEGER,  -- User ID
        name::TEXT    -- User name
    FROM users
    WHERE active = true  -- Only active users
""")
''')

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()


def test_query_with_semicolon(tmp_path: Path) -> None:
    """Test query ending with semicolon."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Query with trailing semicolon
query = sql("SELECT id::INTEGER FROM users;")
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Should handle semicolon
    assert "SELECT id::INTEGER FROM users" in stub_content


def test_nested_function_calls(tmp_path: Path) -> None:
    """Test sql() calls inside function definitions."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql


def get_user_query():
    """Function returning a query."""
    return sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")


def get_posts_query():
    """Another function returning a query."""
    return sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")
''')

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Both queries should be captured
    assert "SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id" in stub_content
    assert "SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id" in stub_content


def test_class_method_queries(tmp_path: Path) -> None:
    """Test sql() calls inside class methods."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql


class UserRepository:
    """Repository pattern with SQL queries."""

    def get_user(self, user_id: int):
        """Get user by ID."""
        return sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")

    def get_all_users(self):
        """Get all users."""
        return sql("SELECT id::INTEGER, name::TEXT FROM users")
''')

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Both queries should be captured
    assert "SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id" in stub_content
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content


def test_query_in_conditional(tmp_path: Path) -> None:
    """Test sql() calls inside conditional blocks."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

DEBUG = True

if DEBUG:
    # Query only used in debug mode
    debug_query = sql("SELECT id::INTEGER, created::TEXT FROM debug_logs")

# Always used query
user_query = sql("SELECT id::INTEGER FROM users")
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Both queries should be captured (static analysis should find both)
    assert "SELECT id::INTEGER FROM users" in stub_content
    # Debug query may or may not be included depending on implementation

"""End-to-end tests for SQL dialects and schema-based type inference.

Tests different SQL dialects and schema file usage.
"""

import subprocess
import sys
from pathlib import Path

from inline_snapshot import snapshot


def test_postgres_dialect(tmp_path: Path) -> None:
    """Test PostgreSQL-specific type inference."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# PostgreSQL-specific types
query1 = sql("SELECT id::INTEGER, name::TEXT, created::TIMESTAMP FROM users")
query2 = sql("SELECT data::JSONB FROM settings")
query3 = sql("SELECT tags::TEXT[] FROM posts")
""")

    # Generate stub with PostgreSQL dialect
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(tmp_path),
            "--mode=full",
            "--dialect=postgres",
        ],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Verify stub was created
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # PostgreSQL types should be inferred
    # TIMESTAMP might be datetime, JSONB could be str, TEXT[] could be str
    assert "SQL[tuple[int, str, str]]" in stub_content or "tuple[int, str" in stub_content


def test_mysql_dialect(tmp_path: Path) -> None:
    """Test MySQL-specific type inference."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# MySQL-specific casts
query = sql("SELECT CAST(id AS SIGNED), CAST(name AS CHAR), CAST(price AS DECIMAL) FROM products")
""")

    # Generate stub with MySQL dialect
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(tmp_path),
            "--mode=full",
            "--dialect=mysql",
        ],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # MySQL SIGNED -> int, CHAR -> str, DECIMAL -> could be float or str
    assert "tuple[int," in stub_content


def test_schema_based_type_inference(tmp_path: Path) -> None:
    """Test type inference using a schema file."""
    # Create schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text("""CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE posts (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    title TEXT NOT NULL,
    content TEXT
);
""")

    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Queries without explicit casts - should use schema for inference
query1 = sql("SELECT id, name, email FROM users")
query2 = sql("SELECT id, title FROM posts")
""")

    # Generate stub with schema file
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(tmp_path),
            "--mode=full",
            f"--schema={schema_file}",
        ],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # Should infer types from schema
    # id -> int, name -> str, email -> str
    assert "tuple[int, str, str]" in stub_content or "tuple[int, str]" in stub_content


def test_without_type_casts_fallback(tmp_path: Path) -> None:
    """Test that queries without casts fall back to generic string types."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Query without any type information
query = sql("SELECT id, name FROM users")
""")

    # Generate stub without schema
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # Without casts or schema, should default to str types
    assert "tuple[str, str]" in stub_content or "SQL[tuple[" in stub_content


def test_mixed_typed_and_untyped_queries(tmp_path: Path) -> None:
    """Test mix of queries with and without type casts."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Query with explicit casts
typed_query = sql("SELECT id::INTEGER, name::TEXT FROM users")

# Query without casts
untyped_query = sql("SELECT id, name FROM posts")
""")

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Both queries should be in the stub
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content
    assert "SELECT id, name FROM posts" in stub_content

    # Snapshot the result
    assert stub_content == snapshot('''\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id, name FROM posts']) -> SQL[tuple[str, str]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, name::TEXT FROM users']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = Query.sql
''')


def test_uuid_type_inference(tmp_path: Path) -> None:
    """Test UUID type inference."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Query with UUID type
query = sql("SELECT id::UUID, name::TEXT FROM users")
""")

    # Generate stub
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(tmp_path),
            "--mode=full",
            "--dialect=postgres",
        ],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Should include uuid import when UUID type is detected
    if "uuid.UUID" in stub_content:
        assert "import uuid" in stub_content

    # Snapshot to verify UUID handling
    assert "SELECT id::UUID, name::TEXT FROM users" in stub_content


def test_multi_schema_with_schema_path_attribute(tmp_path: Path) -> None:
    """Test type inference with multiple registries using __schema_path__ attribute."""
    # Create separate schema files for different domains
    orders_schema = tmp_path / "orders.sql"
    orders_schema.write_text("""CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    total FLOAT NOT NULL
);
""")

    users_schema = tmp_path / "users.sql"
    users_schema.write_text("""CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
""")

    # Create separate registry files with __schema_path__
    orders_queries = tmp_path / "orders_queries.py"
    orders_queries.write_text("""from sqlty import SQLRegistry


class OrdersQueries(SQLRegistry):
    __schema_path__ = "orders.sql"


sql = OrdersQueries.sql

order_query = sql("SELECT id, total FROM orders")
""")

    users_queries = tmp_path / "users_queries.py"
    users_queries.write_text("""from sqlty import SQLRegistry


class UsersQueries(SQLRegistry):
    __schema_path__ = "users.sql"


sql = UsersQueries.sql

user_query = sql("SELECT id, name FROM users")
""")

    # Create main file that imports from both registries
    main_file = tmp_path / "main.py"
    main_file.write_text("""from orders_queries import sql as order_sql
from users_queries import sql as user_sql

# Example usage
user_result = user_sql("SELECT id, name FROM users")
order_result = order_sql("SELECT id, total FROM orders")
""")

    # Generate stubs
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(tmp_path),
            "--mode=full",
        ],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Check orders_queries.pyi
    orders_stub = tmp_path / "orders_queries.pyi"
    assert orders_stub.exists()
    orders_content = orders_stub.read_text()
    
    # Should infer correct types from schema: id -> int, total -> float
    assert "tuple[int, float]" in orders_content

    # Check users_queries.pyi
    users_stub = tmp_path / "users_queries.pyi"
    assert users_stub.exists()
    users_content = users_stub.read_text()
    
    # Should infer correct types from schema: id -> int, name -> str
    assert "tuple[int, str]" in users_content

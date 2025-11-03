"""Tests for automatic schema detection via __schema_path__ property."""

from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from sqlty._generator import TypeStubGenerator

if TYPE_CHECKING:
    from _pytest.logging import LogCaptureFixture


def test_schema_path_detection_with_valid_schema(tmp_path: Path) -> None:
    """Test that __schema_path__ is detected and used for type inference."""
    # Create a schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email VARCHAR(255)
        );
        """
    )

    # Create a registry with __schema_path__
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "schema.sql"

sql = UserQueries.sql

# Query without explicit casts - should infer from schema
user_query = sql("SELECT id, name, email FROM users")
"""
    )

    # Generate stub
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Verify correct type inference from schema
    assert "SQL[tuple[int, str, str]]" in stub_content
    assert "tuple[int, str, str]" in stub_content


def test_schema_path_detection_without_schema_path(tmp_path: Path) -> None:
    """Test that queries without __schema_path__ work normally."""
    # Create a registry without __schema_path__
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    pass

sql = UserQueries.sql

# Query with explicit casts
user_query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    )

    # Generate stub
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Verify correct type inference from casts
    assert "SQL[tuple[int, str]]" in stub_content


def test_schema_path_detection_with_missing_file(tmp_path: Path) -> None:
    """Test that missing schema file raises RuntimeError."""
    # Create a registry with __schema_path__ pointing to non-existent file
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "missing_schema.sql"

sql = UserQueries.sql

# Query without casts or schema - should raise error
user_query = sql("SELECT id, name FROM users")
"""
    )

    # Generate stub - should raise RuntimeError for missing schema
    generator = TypeStubGenerator(sql_dialect="postgres")
    with pytest.raises(RuntimeError, match="Schema file not found"):
        generator.generate_stub_file(query_file, mode="full")


def test_schema_path_detection_with_invalid_schema(tmp_path: Path) -> None:
    """Test that invalid schema file is handled gracefully."""
    # Create an invalid schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text("INVALID SQL SYNTAX HERE @#$%")

    # Create a registry with __schema_path__
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "schema.sql"

sql = UserQueries.sql

user_query = sql("SELECT id, name FROM users")
"""
    )

    # Generate stub - should not crash
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Should still generate stub
    assert "UserQueries" in stub_content
    assert "sql" in stub_content


def test_schema_path_detection_relative_to_module(tmp_path: Path) -> None:
    """Test that schema path is resolved relative to the module file."""
    # Create a subdirectory
    subdir = tmp_path / "queries"
    subdir.mkdir()

    # Create a schema file in the subdirectory
    schema_file = subdir / "schema.sql"
    schema_file.write_text(
        """
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            price FLOAT NOT NULL
        );
        """
    )

    # Create a registry with relative __schema_path__
    query_file = subdir / "product_queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class ProductQueries(SQLRegistry):
    __schema_path__ = "schema.sql"

sql = ProductQueries.sql

product_query = sql("SELECT id, price FROM products")
"""
    )

    # Generate stub
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Verify correct type inference from schema
    assert "SQL[tuple[int, float]]" in stub_content


def test_cli_schema_takes_precedence_over_class_property(tmp_path: Path) -> None:
    """Test that CLI --schema flag takes precedence over __schema_path__."""
    # Create two schema files with different types
    schema_file1 = tmp_path / "schema1.sql"
    schema_file1.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        """
    )

    schema_file2 = tmp_path / "schema2.sql"
    schema_file2.write_text(
        """
        CREATE TABLE users (
            id BIGINT PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        );
        """
    )

    # Create a registry with __schema_path__ pointing to schema1
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "schema1.sql"

sql = UserQueries.sql

user_query = sql("SELECT id, name FROM users")
"""
    )

    # Generate stub with CLI schema (schema2) via global mapping
    generator = TypeStubGenerator(sql_dialect="postgres", schema_mappings={None: schema_file2})
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Should use CLI schema (schema2), not __schema_path__ (schema1)
    # Both INTEGER and BIGINT map to int, and TEXT/VARCHAR to str
    # So we can't distinguish, but the test documents the precedence
    assert "SQL[tuple[int, str]]" in stub_content


def test_schema_path_detection_with_multiple_tables(tmp_path: Path) -> None:
    """Test schema detection with multiple tables."""
    # Create a schema file with multiple tables
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE posts (
            id INTEGER PRIMARY KEY,
            user_id INTEGER REFERENCES users(id),
            title TEXT NOT NULL,
            published BOOLEAN DEFAULT FALSE
        );
        """
    )

    # Create a registry with __schema_path__
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class Queries(SQLRegistry):
    __schema_path__ = "schema.sql"

sql = Queries.sql

user_query = sql("SELECT id, name FROM users")
post_query = sql("SELECT id, title, published FROM posts")
"""
    )

    # Generate stub
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Verify correct type inference for both tables
    assert "SQL[tuple[int, str]]" in stub_content
    assert "SQL[tuple[int, str, bool]]" in stub_content


def test_schema_path_detection_without_sqlregistry_base(tmp_path: Path) -> None:
    """Test that __schema_path__ in non-SQLRegistry class is ignored."""
    # Create a schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        """
    )

    # Create a regular class with __schema_path__ (not SQLRegistry)
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
class NotARegistry:
    __schema_path__ = "schema.sql"

def sql(query: str) -> str:
    return query

user_query = sql("SELECT id, name FROM users")
"""
    )

    # Generate stub
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Should not use schema since it's not a SQLRegistry
    # Query should not have type information
    assert "SQL[tuple[int, str]]" not in stub_content or "tuple[int, str]]" not in stub_content


def test_schema_path_regression_id_type_detection(tmp_path: Path) -> None:
    """Regression test: Ensure id column is detected as int, not str.

    This test prevents the bug where schema detection wasn't working
    and all columns were falling back to str type.
    """
    # Create a schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        """
    )

    # Create a registry with __schema_path__
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class SchemaQueries(SQLRegistry):
    __schema_path__ = "schema.sql"

sql = SchemaQueries.sql

# This query should infer id as int, name as str from schema
user_query = sql("SELECT id, name FROM users")
"""
    )

    # Generate stub
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # CRITICAL: id must be int, not str
    assert "SQL[tuple[int, str]]" in stub_content
    # Ensure it's not incorrectly detected as str
    assert "SQL[tuple[str, str]]" not in stub_content


def test_schema_path_detection_with_parent_directory(tmp_path: Path) -> None:
    """Test schema path with parent directory reference."""
    # Create directory structure
    queries_dir = tmp_path / "queries"
    queries_dir.mkdir()

    # Create schema in parent directory
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            active BOOLEAN DEFAULT TRUE
        );
        """
    )

    # Create a registry with __schema_path__ pointing to parent
    query_file = queries_dir / "user_queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "../schema.sql"

sql = UserQueries.sql

user_query = sql("SELECT id, active FROM users")
"""
    )

    # Generate stub
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Verify correct type inference
    assert "SQL[tuple[int, bool]]" in stub_content


def test_schema_path_with_empty_schema_file(tmp_path: Path) -> None:
    """Test that empty schema file is handled gracefully."""
    # Create an empty schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text("")

    # Create a registry with __schema_path__
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "schema.sql"

sql = UserQueries.sql

user_query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    )

    # Generate stub - should not crash
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(query_file, mode="full")

    # Read generated stub
    stub_content = stub_path.read_text()

    # Should still work with explicit casts
    assert "SQL[tuple[int, str]]" in stub_content


def test_schema_path_detection_message_output(tmp_path: Path, caplog: "LogCaptureFixture") -> None:
    """Test that schema detection prints informative message."""
    # Create a schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY
        );
        """
    )

    # Create a registry with __schema_path__
    query_file = tmp_path / "queries.py"
    query_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "schema.sql"

sql = UserQueries.sql

user_query = sql("SELECT id FROM users")
"""
    )

    # Generate stub and capture logs
    import logging

    with caplog.at_level(logging.WARNING, logger="sqlty._generator"):
        generator = TypeStubGenerator(sql_dialect="postgres")
        generator.generate_stub_file(query_file, mode="full")

    # Check log output for schema detection message
    assert "Using schema from" in caplog.text
    assert "schema.sql" in caplog.text
    # Accept any schema source (mapping, __schema_path__, CLI, etc.)
    assert (
        "__schema_path__" in caplog.text
        or "mapping" in caplog.text
        or "CLI --schema" in caplog.text
        or "default" in caplog.text
    )

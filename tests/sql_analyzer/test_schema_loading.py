"""Tests for schema loading and type inference enhancement."""

import tempfile
from pathlib import Path

import pytest

from sqlty._sql_analyzer import SQLAnalyzer


def test_schema_loading_basic() -> None:
    """Test that schema file is loaded and parsed correctly."""
    schema_content = """
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        age SMALLINT,
        balance DECIMAL(10, 2)
    );
    """

    with tempfile.NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write(schema_content)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Check that schema was loaded
        assert "users" in analyzer.schema
        assert analyzer.schema["users"]["id"] == "INT"
        assert analyzer.schema["users"]["name"] == "TEXT"
        assert analyzer.schema["users"]["age"] == "SMALLINT"
        assert analyzer.schema["users"]["balance"] == "DECIMAL(10, 2)"
    finally:
        schema_path.unlink()


def test_schema_loading_multiple_tables() -> None:
    """Test schema loading with multiple tables."""
    schema_content = """
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL
    );

    CREATE TABLE posts (
        id INTEGER PRIMARY KEY,
        user_id INTEGER,
        title VARCHAR(255),
        created_at TIMESTAMP
    );
    """

    with tempfile.NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write(schema_content)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Check both tables were loaded
        assert "users" in analyzer.schema
        assert "posts" in analyzer.schema
        assert analyzer.schema["users"]["id"] == "INT"
        assert analyzer.schema["posts"]["title"] == "VARCHAR(255)"
        assert analyzer.schema["posts"]["created_at"] == "TIMESTAMP"
    finally:
        schema_path.unlink()


def test_type_inference_with_schema() -> None:
    """Test that schema enhances type inference for SELECT queries."""
    schema_content = """
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        age SMALLINT
    );
    """

    with tempfile.NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write(schema_content)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Test simple SELECT with schema
        sql = "SELECT id, name, age FROM users"
        result = analyzer.analyze_select(sql)

        assert result is not None
        assert len(result) == 3

        # Check that types were inferred from schema
        assert result[0].name == "id"
        assert result[0].python_type == "int"
        assert result[0].sql_type == "INT"

        assert result[1].name == "name"
        assert result[1].python_type == "str"
        assert result[1].sql_type == "TEXT"

        assert result[2].name == "age"
        assert result[2].python_type == "int"
        assert result[2].sql_type == "SMALLINT"
    finally:
        schema_path.unlink()


def test_type_inference_without_schema() -> None:
    """Test that queries work without schema (no type inference)."""
    analyzer = SQLAnalyzer()  # No schema

    sql = "SELECT id, name FROM users"
    result = analyzer.analyze_select(sql)

    assert result is not None
    assert len(result) == 2

    # Without schema, python_type should be None
    assert result[0].name == "id"
    assert result[0].python_type is None

    assert result[1].name == "name"
    assert result[1].python_type is None


def test_generate_return_type_with_schema() -> None:
    """Test return type generation uses schema information."""
    schema_content = """
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        score FLOAT
    );
    """

    with tempfile.NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write(schema_content)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Test return type generation with schema
        sql = "SELECT id, name, score FROM users"
        return_type = analyzer.generate_return_type(sql)

        # Should get precise types from schema
        assert return_type == "tuple[int, str, float]"
    finally:
        schema_path.unlink()


def test_generate_return_type_without_schema() -> None:
    """Test return type generation defaults to str without schema."""
    analyzer = SQLAnalyzer()  # No schema

    sql = "SELECT id, name FROM users"
    return_type = analyzer.generate_return_type(sql)

    # Without schema, should default to str
    assert return_type == "tuple[str, str]"


def test_schema_loading_with_comments() -> None:
    """Test that schema files with SQL comments are parsed correctly."""
    schema_content = """
    -- Users table
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,  -- Unique identifier
        name TEXT NOT NULL       -- User's full name
    );

    /* Posts table
       for user content */
    CREATE TABLE posts (
        id INTEGER PRIMARY KEY,
        title TEXT
    );
    """

    with tempfile.NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write(schema_content)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Check both tables were loaded despite comments
        assert "users" in analyzer.schema
        assert "posts" in analyzer.schema
        assert analyzer.schema["users"]["name"] == "TEXT"
        assert analyzer.schema["posts"]["title"] == "TEXT"
    finally:
        schema_path.unlink()


def test_schema_loading_invalid_path() -> None:
    """Test that invalid schema path raises RuntimeError."""
    # Should raise RuntimeError when schema file doesn't exist
    with pytest.raises(RuntimeError, match="Schema file not found"):
        SQLAnalyzer(schema_path=Path("/nonexistent/schema.sql"))


def test_schema_with_qualified_table_name() -> None:
    """Test SELECT with table-qualified column names."""
    schema_content = """
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL
    );
    """

    with tempfile.NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write(schema_content)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Test with table-qualified columns
        sql = "SELECT users.id, users.name FROM users"
        result = analyzer.analyze_select(sql)

        assert result is not None
        assert len(result) == 2
        assert result[0].python_type == "int"
        assert result[1].python_type == "str"
    finally:
        schema_path.unlink()

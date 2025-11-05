"""Tests for SUM type inference with and without schema."""

from pathlib import Path

import pytest

from sqlty._sql_analyzer import SQLAnalyzer


def test_sum_integer_column_with_schema(tmp_path: Path) -> None:
    """Test SUM of integer column with schema returns int."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)
    result = analyzer.generate_return_type("SELECT SUM(quantity) FROM products")

    # SUM of integer column should return int when schema is available
    assert result == "tuple[int]"


def test_sum_float_column_with_schema(tmp_path: Path) -> None:
    """Test SUM of float column with schema returns float."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)
    result = analyzer.generate_return_type("SELECT SUM(price) FROM products")

    # SUM of float column should return float
    assert result == "tuple[float]"


def test_sum_with_cast_overrides_schema(tmp_path: Path) -> None:
    """Test that explicit cast takes precedence over schema."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)

    # Cast integer column to float - should use cast type
    result1 = analyzer.generate_return_type("SELECT SUM(quantity::FLOAT) FROM products")
    assert result1 == "tuple[float]"

    # Cast float column to integer - should use cast type
    result2 = analyzer.generate_return_type("SELECT SUM(price::INTEGER) FROM products")
    assert result2 == "tuple[int]"


def test_sum_without_schema_defaults_to_float() -> None:
    """Test SUM without schema defaults to float."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("SELECT SUM(amount) FROM orders")

    # Without schema, SUM should default to float
    assert result == "tuple[float]"


def test_sum_mixed_schema_and_cast(tmp_path: Path) -> None:
    """Test mixing schema-inferred and cast-specified types."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)

    # Mix schema inference and explicit cast
    result = analyzer.generate_return_type(
        "SELECT SUM(quantity), SUM(price::INTEGER) FROM products"
    )

    # First SUM uses schema (int), second uses cast (int)
    assert result == "tuple[int, int]"


def test_sum_multiple_columns_from_schema(tmp_path: Path) -> None:
    """Test multiple SUM operations with schema inference."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)
    result = analyzer.generate_return_type("SELECT SUM(quantity), SUM(price) FROM products")

    # First is int, second is float based on schema
    assert result == "tuple[int, float]"


def test_sum_unknown_column_with_schema_raises_error(tmp_path: Path) -> None:
    """Test that SUM of unknown column with schema raises error."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)

    # Query column not in schema should raise error
    with pytest.raises(RuntimeError, match="Column 'unknown_col' not found in table 'products'"):
        analyzer.generate_return_type("SELECT SUM(unknown_col) FROM products")


def test_sum_unknown_column_with_cast_works(tmp_path: Path) -> None:
    """Test that SUM of unknown column with explicit cast works."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)

    # Query column not in schema WITH cast should work
    result = analyzer.generate_return_type("SELECT SUM(unknown_col::INTEGER) FROM products")
    assert result == "tuple[int]"


def test_sum_unknown_table_with_schema_uses_fallback(tmp_path: Path) -> None:
    """Test that SUM with unknown table falls back to float."""
    # Create a temporary schema file
    schema_path = tmp_path / "schema.sql"
    schema_path.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
    """)

    analyzer = SQLAnalyzer(schema_path=schema_path)

    # Query from table not in schema should fall back to float (table not found in query)
    result = analyzer.generate_return_type("SELECT SUM(amount) FROM orders")
    assert result == "tuple[float]"

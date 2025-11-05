"""Tests for SUM type inference with and without schema."""

from pathlib import Path
from tempfile import NamedTemporaryFile

from sqlty._sql_analyzer import SQLAnalyzer


def test_sum_integer_column_with_schema() -> None:
    """Test SUM of integer column with schema returns int."""
    # Create a temporary schema file
    with NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write("""
            CREATE TABLE products (
                id INTEGER PRIMARY KEY,
                quantity INTEGER NOT NULL,
                price FLOAT NOT NULL
            );
        """)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)
        result = analyzer.generate_return_type("SELECT SUM(quantity) FROM products")

        # SUM of integer column should return int when schema is available
        assert result == "tuple[int]"
    finally:
        schema_path.unlink()


def test_sum_float_column_with_schema() -> None:
    """Test SUM of float column with schema returns float."""
    # Create a temporary schema file
    with NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write("""
            CREATE TABLE products (
                id INTEGER PRIMARY KEY,
                quantity INTEGER NOT NULL,
                price FLOAT NOT NULL
            );
        """)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)
        result = analyzer.generate_return_type("SELECT SUM(price) FROM products")

        # SUM of float column should return float
        assert result == "tuple[float]"
    finally:
        schema_path.unlink()


def test_sum_with_cast_overrides_schema() -> None:
    """Test that explicit cast takes precedence over schema."""
    # Create a temporary schema file
    with NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write("""
            CREATE TABLE products (
                id INTEGER PRIMARY KEY,
                quantity INTEGER NOT NULL,
                price FLOAT NOT NULL
            );
        """)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Cast integer column to float - should use cast type
        result1 = analyzer.generate_return_type("SELECT SUM(quantity::FLOAT) FROM products")
        assert result1 == "tuple[float]"

        # Cast float column to integer - should use cast type
        result2 = analyzer.generate_return_type("SELECT SUM(price::INTEGER) FROM products")
        assert result2 == "tuple[int]"
    finally:
        schema_path.unlink()


def test_sum_without_schema_defaults_to_float() -> None:
    """Test SUM without schema defaults to float."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("SELECT SUM(amount) FROM orders")

    # Without schema, SUM should default to float
    assert result == "tuple[float]"


def test_sum_mixed_schema_and_cast() -> None:
    """Test mixing schema-inferred and cast-specified types."""
    # Create a temporary schema file
    with NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write("""
            CREATE TABLE products (
                id INTEGER PRIMARY KEY,
                quantity INTEGER NOT NULL,
                price FLOAT NOT NULL
            );
        """)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)

        # Mix schema inference and explicit cast
        result = analyzer.generate_return_type(
            "SELECT SUM(quantity), SUM(price::INTEGER) FROM products"
        )

        # First SUM uses schema (int), second uses cast (int)
        assert result == "tuple[int, int]"
    finally:
        schema_path.unlink()


def test_sum_multiple_columns_from_schema() -> None:
    """Test multiple SUM operations with schema inference."""
    # Create a temporary schema file
    with NamedTemporaryFile(mode="w", suffix=".sql", delete=False) as f:
        f.write("""
            CREATE TABLE products (
                id INTEGER PRIMARY KEY,
                quantity INTEGER NOT NULL,
                price FLOAT NOT NULL
            );
        """)
        schema_path = Path(f.name)

    try:
        analyzer = SQLAnalyzer(schema_path=schema_path)
        result = analyzer.generate_return_type("SELECT SUM(quantity), SUM(price) FROM products")

        # First is int, second is float based on schema
        assert result == "tuple[int, float]"
    finally:
        schema_path.unlink()

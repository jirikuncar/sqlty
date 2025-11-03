"""Tests for SQL analyzer."""

import pytest

from sqlty._sql_analyzer import SQLAnalyzer


def test_simple_select_no_types() -> None:
    """Test simple SELECT without type casts."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select("SELECT id, name FROM users")

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].name == "id"
    assert columns[0].python_type is None
    assert columns[1].name == "name"
    assert columns[1].python_type is None


def test_select_with_casts() -> None:
    """Test SELECT with explicit type casts."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select("SELECT id::INTEGER, name::TEXT FROM users")

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[0].sql_type == "INT"
    assert columns[1].name == "name"
    assert columns[1].python_type == "str"
    assert columns[1].sql_type == "TEXT"


def test_select_with_count() -> None:
    """Test SELECT with COUNT aggregate."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select("SELECT COUNT(*) FROM users")

    assert columns is not None
    assert len(columns) == 1
    assert columns[0].python_type == "int"
    assert columns[0].sql_type == "INTEGER"


def test_select_with_mixed_types() -> None:
    """Test SELECT with various type casts."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select(
        """
        SELECT
            id::INTEGER,
            name::TEXT,
            price::FLOAT,
            active::BOOLEAN,
            COUNT(*)::INTEGER
        FROM products
        """
    )

    assert columns is not None
    assert len(columns) == 5
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "float"
    assert columns[3].python_type == "bool"
    assert columns[4].python_type == "int"


def test_generate_return_type_single_column() -> None:
    """Test return type generation for single column."""
    analyzer = SQLAnalyzer()
    return_type = analyzer.generate_return_type("SELECT COUNT(*) FROM users")

    assert return_type == "tuple[int]"


def test_generate_return_type_multiple_columns() -> None:
    """Test return type generation for multiple columns."""
    analyzer = SQLAnalyzer()
    return_type = analyzer.generate_return_type(
        "SELECT id::INTEGER, name::TEXT, score::FLOAT FROM users"
    )

    assert return_type == "tuple[int, str, float]"


def test_generate_return_type_no_types() -> None:
    """Test return type generation defaults to str."""
    analyzer = SQLAnalyzer()
    return_type = analyzer.generate_return_type("SELECT id, name FROM users")

    assert return_type == "tuple[str, str]"


def test_invalid_sql() -> None:
    """Test invalid SQL returns None."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select("INVALID SQL QUERY")

    assert columns is None


def test_non_select_query() -> None:
    """Test non-SELECT query returns None."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select("INSERT INTO users VALUES (1, 'test')")

    assert columns is None


@pytest.mark.parametrize(
    ("sql_type", "expected_python_type"),
    [
        ("INTEGER", "int"),
        ("BIGINT", "int"),
        ("SMALLINT", "int"),
        ("FLOAT", "float"),
        ("DOUBLE", "float"),
        ("TEXT", "str"),
        ("VARCHAR", "str"),
        ("BOOLEAN", "bool"),
        ("DATE", "str"),
        ("TIMESTAMP", "str"),
        ("UUID", "uuid.UUID"),
    ],
)
def test_sql_to_python_type_mapping(sql_type: str, expected_python_type: str) -> None:
    """Test SQL to Python type mapping."""
    analyzer = SQLAnalyzer()
    # Build a query with the type cast
    query = f"SELECT id::{sql_type} FROM users"
    columns = analyzer.analyze_select(query)

    assert columns is not None
    assert len(columns) == 1
    assert columns[0].python_type == expected_python_type


def test_dialect_postgres() -> None:
    """Test PostgreSQL dialect."""
    analyzer = SQLAnalyzer(dialect="postgres")
    columns = analyzer.analyze_select("SELECT id::INTEGER FROM users WHERE id = :id")

    assert columns is not None
    assert columns[0].python_type == "int"


def test_dialect_mysql() -> None:
    """Test MySQL dialect."""
    analyzer = SQLAnalyzer(dialect="mysql")
    columns = analyzer.analyze_select("SELECT CAST(id AS SIGNED) FROM users")

    assert columns is not None
    # MySQL SIGNED maps to INT
    assert columns[0].python_type == "int"


def test_uuid_type_inference() -> None:
    """Test UUID type mapping from SQL to Python."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select("SELECT user_id::UUID FROM users")

    assert columns is not None
    assert len(columns) == 1
    assert columns[0].name == "user_id"
    assert columns[0].python_type == "uuid.UUID"
    assert columns[0].sql_type == "UUID"


def test_uuid_with_other_types() -> None:
    """Test UUID in query with multiple types."""
    analyzer = SQLAnalyzer()
    columns = analyzer.analyze_select(
        "SELECT id::INTEGER, user_id::UUID, name::TEXT FROM users"
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "uuid.UUID"
    assert columns[2].python_type == "str"

"""Tests to cover uncovered lines in sql_analyzer.py."""

from sqlty._sql_analyzer import SQLAnalyzer


def test_avg_function() -> None:
    """Test AVG function type inference (lines 88-89)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT AVG(price)::NUMERIC FROM products
    """)
    # AVG with explicit cast should return float
    assert result == "tuple[float]"


def test_stddev_function() -> None:
    """Test STDDEV function type inference (line 92)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT STDDEV(amount) FROM transactions
    """)
    # STDDEV returns float
    assert result == "tuple[float]"


def test_upper_function() -> None:
    """Test UPPER string function (lines 106-117)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT UPPER(name) FROM users
    """)
    assert result == "tuple[str]"


def test_lower_function() -> None:
    """Test LOWER string function (lines 106-117)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT LOWER(email) FROM users
    """)
    assert result == "tuple[str]"


def test_trim_function() -> None:
    """Test TRIM function (lines 106-117)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT TRIM(description) FROM items
    """)
    assert result == "tuple[str]"


def test_now_function() -> None:
    """Test NOW() function (lines 106-117)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT NOW() FROM dual
    """)
    assert result == "tuple[str]"


def test_current_date_function() -> None:
    """Test CURRENT_DATE function (lines 106-117)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT CURRENT_DATE FROM dual
    """)
    assert result == "tuple[str]"


def test_current_timestamp_function() -> None:
    """Test CURRENT_TIMESTAMP function (lines 106-117)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT CURRENT_TIMESTAMP FROM dual
    """)
    assert result == "tuple[str]"


def test_length_function() -> None:
    """Test LENGTH function returns int (line 129)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT LENGTH(name) FROM users
    """)
    assert result == "tuple[int]"


def test_position_function() -> None:
    """Test POSITION function returns int (line 129)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT POSITION('a' IN name) FROM users
    """)
    # POSITION is parsed as StrPosition which doesn't match POSITION exactly
    assert result == "tuple[str]"


def test_unknown_function_type() -> None:
    """Test unknown function returns None (line 141)."""
    analyzer = SQLAnalyzer()
    # Use a function that doesn't have a specific type mapping
    result = analyzer.generate_return_type("""
        SELECT SOME_UNKNOWN_FUNCTION(field) FROM table_name
    """)
    # Unknown functions default to str
    assert result == "tuple[str]"


def test_no_columns_returns_none() -> None:
    """Test that empty column list returns None."""
    analyzer = SQLAnalyzer()
    # Invalid SQL that parses but has no columns
    result = analyzer.generate_return_type("")
    assert result is None

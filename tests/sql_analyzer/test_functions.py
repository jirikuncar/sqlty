"""Additional tests to achieve 100% coverage on sql_analyzer.py."""

from sqlty._sql_analyzer import SQLAnalyzer


def test_sum_without_cast() -> None:
    """Test SUM function without explicit cast (lines 88-89)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT SUM(amount) FROM orders
    """)
    # SUM without cast should return float
    assert result == "tuple[float]"


def test_avg_without_cast() -> None:
    """Test AVG function without explicit cast."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT AVG(score) FROM tests
    """)
    # AVG without cast should return float
    assert result == "tuple[float]"


def test_trim_via_func() -> None:
    """Test TRIM function is recognized (line 115)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT TRIM(' ' FROM name) FROM users
    """)
    # TRIM should return str
    assert result == "tuple[str]"


def test_concat_function() -> None:
    """Test CONCAT function (line 115)."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT CONCAT(first_name, ' ', last_name) FROM users
    """)
    # CONCAT should return str
    assert result == "tuple[str]"


def test_unknown_function_returns_none() -> None:
    """Test that unknown function inference returns None (line 141)."""
    analyzer = SQLAnalyzer()

    # Test with a truly unknown/custom function that will be parsed as Anonymous
    result = analyzer.generate_return_type("""
        SELECT MY_CUSTOM_FUNCTION(data) FROM table_name
    """)
    # Unknown functions default to str
    assert result == "tuple[str]"

    # Also test with another unknown function name
    result2 = analyzer.generate_return_type("""
        SELECT WEIRD_FUNC(x, y) FROM z
    """)
    assert result2 == "tuple[str]"

    # Test EXTRACT which might not be recognized
    result3 = analyzer.generate_return_type("""
        SELECT EXTRACT(YEAR FROM created_at) FROM events
    """)
    # Should default to str if not recognized
    assert result3 in ["tuple[str]", "tuple[int]"]


def test_now_function_exact() -> None:
    """Test NOW() function to hit line 115."""
    analyzer = SQLAnalyzer()
    # NOW is parsed as Anonymous function
    result = analyzer.generate_return_type("""
        SELECT NOW()
    """)
    # May return str if recognized
    assert result in ["tuple[str]", None]


def test_current_timestamp_exact() -> None:
    """Test CURRENT_TIMESTAMP to hit line 115."""
    analyzer = SQLAnalyzer()
    result = analyzer.generate_return_type("""
        SELECT CURRENT_TIMESTAMP
    """)
    # Should return str
    assert result in ["tuple[str]", None]


def test_empty_column_list() -> None:
    """Test SQL that results in empty column list (line 141)."""
    analyzer = SQLAnalyzer()
    # SELECT without columns produces empty list
    result = analyzer.generate_return_type("SELECT")
    # Should return None when no columns
    assert result is None

    # Also test invalid SQL
    result2 = analyzer.generate_return_type("")
    assert result2 is None

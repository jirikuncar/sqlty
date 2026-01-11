"""Tests for the magic module and --sql string prefix transformation."""

from sqlty._decode import _infer_sql_type, _transform_sql_string


class TestInferSQLType:
    """Test SQL type inference."""

    def test_simple_select_with_cast(self):
        """Test type inference for SELECT with explicit cast."""
        sql = """--sql
SELECT 1::INTEGER
"""
        result = _infer_sql_type(sql)
        assert result == "tuple[int]"

    def test_select_multiple_columns(self):
        """Test type inference for SELECT with multiple columns."""
        sql = """--sql
SELECT id::INTEGER, name::TEXT
"""
        result = _infer_sql_type(sql)
        assert result == "tuple[int, str]"

    def test_select_without_prefix(self):
        """Test type inference for SQL without --sql prefix."""
        sql = """SELECT 1::INTEGER
"""
        result = _infer_sql_type(sql)
        assert result == "tuple[int]"

    def test_invalid_sql(self):
        """Test that invalid SQL returns Any."""
        sql = "--sql NOT VALID SQL"
        result = _infer_sql_type(sql)
        assert result == "Any"


class TestTransformSQLString:
    """Test string transformation."""

    def test_double_quoted_string(self):
        """Test transformation of double-quoted string."""
        string_literal = '"""--sql\nSELECT 1::INTEGER\n"""'
        result = _transform_sql_string(string_literal)
        assert result.startswith("SQL[tuple[int]](")
        assert '"""--sql\nSELECT 1::INTEGER\n"""' in result

    def test_single_quoted_string(self):
        """Test transformation of single-quoted string."""
        string_literal = "'''--sql\nSELECT 1::INTEGER\n'''"
        result = _transform_sql_string(string_literal)
        assert result.startswith("SQL[tuple[int]](")
        assert "'''--sql\nSELECT 1::INTEGER\n'''" in result

    def test_triple_quoted_string(self):
        """Test transformation of triple-quoted string."""
        string_literal = '"""--sql\nSELECT 1::INTEGER\n"""'
        result = _transform_sql_string(string_literal)
        assert result.startswith("SQL[tuple[int]](")
        assert '"""--sql\nSELECT 1::INTEGER\n"""' in result

    def test_non_sql_string(self):
        """Test that non-SQL strings are not transformed."""
        string_literal = '"Hello, World!"'
        result = _transform_sql_string(string_literal)
        assert result == string_literal

    def test_sql_with_whitespace(self):
        """Test SQL with leading/trailing whitespace."""
        string_literal = '"""  --sql\nSELECT 1::INTEGER\n  """'
        result = _transform_sql_string(string_literal)
        # Should detect --sql prefix after stripping and infer type correctly
        assert result.startswith("SQL[tuple[int]](")

    def test_multiple_columns(self):
        """Test SQL with multiple columns."""
        string_literal = '"""--sql\nSELECT id::INTEGER, name::TEXT\n"""'
        result = _transform_sql_string(string_literal)
        assert result.startswith("SQL[tuple[int, str]](")

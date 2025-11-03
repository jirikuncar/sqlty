"""Tests for type stub generator."""

from pathlib import Path

import pytest

from sqlty._generator import TypeStubGenerator

# File Analysis Tests


def test_analyze_simple_literal(tmp_path: Path) -> None:
    """Test analyzing simple SQL literal."""
    source = """
from sqlty import sql

query = sql("SELECT * FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)

    assert len(calls) == 1
    assert calls[0]["literal"] == "SELECT * FROM users"


def test_analyze_multiline_literal(tmp_path: Path) -> None:
    """Test analyzing multiline SQL literal."""
    source = '''
from sqlty import sql

query = sql("""
    SELECT id, name
    FROM users
    WHERE active = true
""")
'''
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)

    assert len(calls) == 1
    assert "SELECT id, name" in calls[0]["literal"]
    assert "FROM users" in calls[0]["literal"]


def test_analyze_multiple_calls(tmp_path: Path) -> None:
    """Test analyzing multiple SQL calls."""
    source = """
from sqlty import sql

query1 = sql("SELECT * FROM users")
query2 = sql("SELECT * FROM posts")
query3 = sql("SELECT * FROM comments")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)

    assert len(calls) == 3
    assert calls[0]["literal"] == "SELECT * FROM users"
    assert calls[1]["literal"] == "SELECT * FROM posts"
    assert calls[2]["literal"] == "SELECT * FROM comments"


def test_analyze_calls_without_schema(tmp_path: Path) -> None:
    """Test analyzing calls without schema returns generic str types."""
    source = """
from sqlty import SQL, sql

query = sql("SELECT id, name FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)

    assert len(calls) == 1
    assert calls[0]["literal"] == "SELECT id, name FROM users"
    # Without schema or casts, should infer generic str types
    assert "tuple[str, str]" in calls[0]["return_type"]  # type: ignore


def test_ignore_non_literal_calls(tmp_path: Path, caplog: pytest.LogCaptureFixture) -> None:
    """Test that non-literal calls are ignored with warning."""
    source = """
from sqlty import sql

dynamic = "SELECT * FROM users"
query = sql(dynamic)  # Not a literal
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    with caplog.at_level("WARNING", logger="sqlty._collector"):
        generator = TypeStubGenerator()
        calls = generator.analyze_file(source_file)

    assert len(calls) == 0

    # Verify warning was emitted via logging
    assert "Call to sql() at line 5 uses a non-literal argument" in caplog.text
    assert "Type inference requires literal strings" in caplog.text


# Type Inference Tests


def test_infer_from_sql_casts(tmp_path: Path) -> None:
    """Test type inference from SQL type casts."""
    source = """
from sqlty import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator(sql_dialect="postgres")
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    assert "tuple[int, str]" in stub


# Stub Generation Tests


def test_generate_empty_stub(tmp_path: Path) -> None:
    """Test generating stub with no SQL calls."""
    source = """
def hello() -> str:
    return "Hello, World!"
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    # Should have imports and class definition
    assert "from typing import" in stub
    assert "class Query(SQLRegistry):" in stub
    # No calls means just the generic fallback
    assert stub.count("@overload") == 1  # Just the generic fallback
    assert stub.count("@classmethod") == 1


def test_generate_single_overload(tmp_path: Path) -> None:
    """Test generating stub with single overload."""
    source = """
from sqlty import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    assert stub.count("@overload") >= 1
    assert stub.count("@classmethod") >= 1
    assert "tuple[int, str]" in stub
    assert "class Query(SQLRegistry):" in stub


def test_generate_multiple_overloads(tmp_path: Path) -> None:
    """Test generating stub with multiple overloads."""
    source = """
from sqlty import sql

query1 = sql("SELECT id::INTEGER, name::TEXT FROM users")
query2 = sql("SELECT COUNT(*)::INTEGER FROM users")
query3 = sql("SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    # 3 specific overloads + 1 generic fallback = 4 total
    assert stub.count("@overload") >= 4
    assert stub.count("@classmethod") >= 4
    assert "tuple[int, str]" in stub
    assert "tuple[int]" in stub
    assert "tuple[int, str, str]" in stub
    assert "class Query(SQLRegistry):" in stub


def test_preserve_whitespace_single_line(tmp_path: Path) -> None:
    """Test whitespace preservation for single-line literals."""
    source = """
from sqlty import sql

query = sql("SELECT id, name FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    assert 'Literal["SELECT id, name FROM users"]' in stub


def test_preserve_whitespace_multiline(tmp_path: Path) -> None:
    """Test whitespace preservation for multiline literals."""
    source = '''
from sqlty import sql

query = sql("""
    SELECT id, name
    FROM users
    WHERE active = true
""")
'''
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    # Should preserve exact formatting including newlines and indentation
    assert '"""' in stub
    assert "SELECT id, name" in stub
    assert "FROM users" in stub
    assert "WHERE active = true" in stub


def test_write_stub_file(tmp_path: Path) -> None:
    """Test writing stub to file."""
    source = """
from sqlty import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    stub_file = tmp_path / "test.pyi"

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub_content = generator.generate_stub_content(calls)
    stub_file.write_text(stub_content)

    assert stub_file.exists()
    assert "tuple[int, str]" in stub_file.read_text()
    assert "@overload" in stub_file.read_text()


# Dialect Support Tests


def test_postgres_dialect(tmp_path: Path) -> None:
    """Test PostgreSQL dialect."""
    source = """
from sqlty import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator(sql_dialect="postgres")
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    assert "tuple[int, str]" in stub


def test_mysql_dialect(tmp_path: Path) -> None:
    """Test MySQL dialect."""
    source = """
from sqlty import sql

query = sql("SELECT CAST(id AS SIGNED), CAST(name AS CHAR) FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator(sql_dialect="mysql")
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    # MySQL SIGNED -> int, CHAR -> str
    assert "tuple[int, str]" in stub


# Edge Cases and Error Handling Tests


def test_empty_file(tmp_path: Path) -> None:
    """Test handling empty file."""
    source_file = tmp_path / "test.py"
    source_file.write_text("")

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    # Should generate valid stub with just imports
    assert "from typing import" in stub


def test_syntax_error_file(tmp_path: Path) -> None:
    """Test handling file with syntax errors."""
    source = """
from sqlty import sql

def broken(
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()

    # Should return empty list and print warning
    calls = generator.analyze_file(source_file)
    assert len(calls) == 0


def test_nonexistent_file() -> None:
    """Test handling nonexistent file."""
    generator = TypeStubGenerator()

    # Should return empty list and print warning
    calls = generator.analyze_file(Path("/nonexistent/file.py"))
    assert len(calls) == 0


def test_no_return_type_annotation(tmp_path: Path) -> None:
    """Test handling SQL with no type information."""
    source = """
from sqlty import sql

query = sql("SELECT * FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls)

    # Should still generate stub even without type info (with Any or generic fallback)
    assert "from typing import" in stub


def test_uuid_import_added_when_needed(tmp_path: Path) -> None:
    """Test that uuid import is added when UUID type is used."""
    source = """
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql

query = sql("SELECT user_id::UUID FROM users")
# Type: SQL[tuple[uuid.UUID]]
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Create a schema with UUID type
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text("CREATE TABLE users (user_id UUID);")

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls, class_name="Query")

    # Should include uuid import when UUID type is detected
    assert "import uuid" in stub
    assert "uuid.UUID" in stub


def test_no_uuid_import_when_not_needed(tmp_path: Path) -> None:
    """Test that uuid import is not added when UUID type is not used."""
    source = """
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    stub = generator.generate_stub_content(calls, class_name="Query")

    # Should NOT include uuid import when UUID type is not used
    assert "import uuid" not in stub

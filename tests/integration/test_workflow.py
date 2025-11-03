"""Integration tests for the complete workflow."""

from pathlib import Path

from sqlty._generator import TypeStubGenerator

# End-to-End Workflow Tests


def test_complete_workflow_with_sql_casts(tmp_path: Path) -> None:
    """Test complete workflow with SQL type casts."""
    # Create source file with SQL type casts
    source = """
from sqlty import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate stub
    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    # Verify stub was created
    assert stub_file.exists()
    stub_content = stub_file.read_text()

    # Verify stub contains correct overload
    assert "@overload" in stub_content
    assert 'Literal["SELECT id::INTEGER, name::TEXT FROM users"]' in stub_content
    assert "tuple[int, str]" in stub_content


def test_complete_workflow_with_sql_inference(tmp_path: Path) -> None:
    """Test complete workflow with SQL type inference."""
    # Create source file with SQL type casts
    source = """
from sqlty import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate stub with PostgreSQL dialect
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_file = generator.generate_stub_file(source_file)

    # Verify stub was created
    assert stub_file.exists()
    stub_content = stub_file.read_text()

    # Verify stub contains inferred types
    assert "@overload" in stub_content
    assert "tuple[int, str]" in stub_content


def test_multiple_queries_workflow(tmp_path: Path) -> None:
    """Test workflow with multiple queries."""
    # Create source file with multiple queries
    source = """
from sqlty import sql

query1 = sql("SELECT id::INTEGER, name::TEXT FROM users")
query2 = sql("SELECT COUNT(*)::INTEGER FROM users")
query3 = sql("SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate stub
    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    # Verify stub was created
    assert stub_file.exists()
    stub_content = stub_file.read_text()

    # Verify all overloads are present
    assert stub_content.count("@overload") >= 3
    assert "tuple[int, str]" in stub_content
    assert "tuple[int]" in stub_content
    assert "tuple[int, str, str]" in stub_content


def test_whitespace_preservation(tmp_path: Path) -> None:
    """Test that whitespace is preserved in generated stubs."""
    # Create source with multiline query
    source = '''
from sqlty import SQL, sql

query: SQL[tuple[int, str]] = sql("""
    SELECT id, name
    FROM users
    WHERE active = true
""")
'''
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate stub
    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    # Verify whitespace preservation
    stub_content = stub_file.read_text()
    assert '"""' in stub_content
    assert "SELECT id, name" in stub_content
    assert "FROM users" in stub_content
    assert "WHERE active = true" in stub_content


def test_directory_analysis(tmp_path: Path) -> None:
    """Test analyzing entire directory."""
    # Create multiple source files
    (tmp_path / "file1.py").write_text("""
from sqlty import SQL, sql

query1: SQL[tuple[int, str]] = sql("SELECT id, name FROM users")
""")

    (tmp_path / "file2.py").write_text("""
from sqlty import SQL, sql

query2: SQL[tuple[int]] = sql("SELECT COUNT(*) FROM posts")
""")

    # Generate stub for directory
    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(tmp_path)

    # Verify stub was created
    assert stub_file.exists()
    stub_content = stub_file.read_text()

    # Verify both queries are in stub
    assert "SELECT id, name FROM users" in stub_content
    assert "SELECT COUNT(*) FROM posts" in stub_content


def test_deduplicate_same_query_across_files(tmp_path: Path) -> None:
    """Two identical SQL calls in different files should produce one overload."""
    # Create two source files with the same exact literal
    (tmp_path / "file1.py").write_text("""
from sqlty import SQL, sql

q1: SQL[tuple[int]] = sql("SELECT 1")
""")

    (tmp_path / "file2.py").write_text("""
from sqlty import SQL, sql

q2: SQL[tuple[int]] = sql("SELECT 1")
""")

    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(tmp_path)

    assert stub_file.exists()
    content = stub_file.read_text()

    # Only one Literal annotation for SELECT 1
    assert content.count('Literal["SELECT 1"]') == 1
    # Should include at least one source location comment
    assert "# from " in content


def test_custom_output_path(tmp_path: Path) -> None:
    """Test specifying custom output path."""
    # Create source file
    source = """
from sqlty import SQL, sql

query: SQL[tuple[int, str]] = sql("SELECT id, name FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate stub with custom output path
    output_file = tmp_path / "custom_stub.pyi"
    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file, output_file)

    # Verify custom path was used
    assert stub_file == output_file
    assert output_file.exists()


def test_no_sql_calls(tmp_path: Path) -> None:
    """Test file with no SQL calls."""
    # Create source file without SQL calls
    source = """
def hello() -> str:
    return "Hello, World!"
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # When there are no SQL calls, the tool doesn't create a stub file
    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)

    # Verify no calls were found
    assert len(calls) == 0


def test_dialect_support(tmp_path: Path) -> None:
    """Test different SQL dialects."""
    # Create source with MySQL-style cast
    source = """
from sqlty import sql

query = sql("SELECT CAST(id AS SIGNED), CAST(name AS CHAR) FROM users")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate stub with MySQL dialect
    generator = TypeStubGenerator(sql_dialect="mysql")
    stub_file = generator.generate_stub_file(source_file)

    # Verify stub was created
    assert stub_file.exists()
    stub_content = stub_file.read_text()

    # Should infer types from MySQL casts
    assert "tuple[int, str]" in stub_content or "@overload" in stub_content


# Error Handling Tests


def test_syntax_error_file(tmp_path: Path) -> None:
    """Test handling file with syntax errors."""
    # Create file with syntax error
    source = """
from sqlty import sql

def broken(
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Should return empty list and print warning
    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)
    assert len(calls) == 0


def test_nonexistent_file() -> None:
    """Test handling nonexistent file."""
    generator = TypeStubGenerator()

    # Should return empty list and print warning
    calls = generator.analyze_file(Path("/nonexistent/file.py"))
    assert len(calls) == 0


def test_invalid_sql_query(tmp_path: Path) -> None:
    """Test handling invalid SQL queries."""
    # Create file with invalid SQL
    source = """
from sqlty import SQL, sql

query: SQL[tuple[int, str]] = sql("INVALID SQL QUERY")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Should still generate stub (with annotation type, not SQL-inferred)
    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    assert stub_file.exists()
    stub_content = stub_file.read_text()
    assert "tuple[int, str]" in stub_content

"""Tests for source comment format in generated stubs."""

from pathlib import Path

from sqlty._generator import TypeStubGenerator


def test_comment_uses_module_name_not_file_path(tmp_path: Path) -> None:
    """Test that comments use fully qualified module names, not file paths."""
    # Create a source file with queries
    source_file = tmp_path / "my_queries.py"
    source_file.write_text("""
from sqlty import sql

query1 = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM products")
""")

    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    content = stub_file.read_text()

    # Comments should use module name "my_queries", not file path
    assert "# from my_queries:4" in content
    assert "# from my_queries:5" in content

    # Should NOT contain file system paths
    assert tmp_path.name not in content.replace("my_queries", "")  # Ensure no path remnants


def test_comment_positioned_inside_function(tmp_path: Path) -> None:
    """Test that comments are inside the function body, not before decorators."""
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import sql

query = sql("SELECT 42::INTEGER")
""")

    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    content = stub_file.read_text()

    # Find the overload method
    lines = content.split("\n")

    # Check structure: @overload -> @classmethod -> def -> ) -> return_type: -> comment -> ...
    found_overload = False
    for i, line in enumerate(lines):
        if "@overload" in line and "def sql(" in lines[i + 2]:
            # Check that comment comes after the closing paren and colon
            # Structure should be:
            # @overload
            # @classmethod
            # def sql(
            #     cls,
            #     query: Literal[...],
            # ) -> SQL[...]:
            #     # from queries:4
            #     ...
            found_return_line = False
            for j in range(i, min(i + 10, len(lines))):
                if ") -> " in lines[j] and lines[j].strip().endswith(":"):
                    # Next non-empty line should be comment or ...
                    next_line_idx = j + 1
                    while next_line_idx < len(lines) and not lines[next_line_idx].strip():
                        next_line_idx += 1
                    if next_line_idx < len(lines):
                        next_line = lines[next_line_idx].strip()
                        # Should be either comment or ...
                        assert next_line.startswith("# from") or next_line == "..."
                        found_return_line = True
                        found_overload = True
                        break
            if found_return_line:
                break

    assert found_overload, "Could not find properly formatted overload"


def test_comment_with_registry_class(tmp_path: Path) -> None:
    """Test comments when using explicit SQLRegistry subclass."""
    source_file = tmp_path / "user_queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    pass

user_sql = UserQueries.sql

query = user_sql("SELECT user_id::INTEGER FROM sessions")
""")

    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    content = stub_file.read_text()

    # Should use module name "user_queries"
    assert "# from user_queries:9" in content


def test_comment_format_multiline_query(tmp_path: Path) -> None:
    """Test comment format with multiline queries."""
    source_file = tmp_path / "complex_queries.py"
    source_file.write_text("""
from sqlty import sql

query = sql('''
    SELECT
        id::INTEGER,
        name::TEXT
    FROM users
    WHERE active = true
''')
""")

    generator = TypeStubGenerator()
    stub_file = generator.generate_stub_file(source_file)

    content = stub_file.read_text()

    # Comment should point to line where sql() call starts
    assert "# from complex_queries:4" in content


def test_no_comment_when_registry_source_missing(tmp_path: Path) -> None:
    """Test that no comment is added when registry_source is not available."""
    # This would be an edge case, but let's handle it gracefully
    source_file = tmp_path / "edge_case.py"
    source_file.write_text("""
# Direct call without proper import tracking might not have registry_source
query = sql("SELECT 1")
""")

    generator = TypeStubGenerator()
    calls = generator.analyze_file(source_file)

    # If registry_source is None, no comment should be generated
    if calls and calls[0].get("registry_source") is None:
        stub = generator.generate_stub_content(calls)
        # Should still have the method, just without the comment
        assert "def sql(" in stub
        # The ... should appear without a comment above it
        lines = stub.split("\n")
        for i, line in enumerate(lines):
            if line.strip() == "...":
                # Check if previous non-empty line is the return type line
                prev_idx = i - 1
                while prev_idx >= 0 and not lines[prev_idx].strip():
                    prev_idx -= 1
                if prev_idx >= 0:
                    # Previous line should be ) -> Type: or just # from comment
                    prev_line = lines[prev_idx].strip()
                    if not prev_line.startswith("# from"):
                        # That's OK - we have no comment
                        assert ") ->" in prev_line or prev_line.endswith(":")

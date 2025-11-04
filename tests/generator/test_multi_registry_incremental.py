"""Test incremental mode with multiple registries in a single file."""

from pathlib import Path

from sqlty._generator import TypeStubGenerator


def test_incremental_mode_with_multiple_registries(tmp_path: Path) -> None:
    """Test that incremental mode works correctly with multiple registries."""
    # Create schema files
    schema_a = tmp_path / "schema_a.sql"
    schema_a.write_text("""
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name VARCHAR(100),
            email VARCHAR(255)
        );
    """)

    schema_b = tmp_path / "schema_b.sql"
    schema_b.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            name VARCHAR(100),
            price FLOAT
        );
    """)

    # Create a module with two registries
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""
from sqlty import SQLRegistry

class QueryA(SQLRegistry):
    __schema_path__ = "schema_a.sql"

class QueryB(SQLRegistry):
    __schema_path__ = "schema_b.sql"
""")

    # First, create a file with one SQL call for QueryA
    usage_file = tmp_path / "usage.py"
    usage_file.write_text("""
from queries import QueryA

query1 = QueryA.sql("SELECT id, name FROM users")
""")

    generator = TypeStubGenerator()

    # Generate initial stub
    stub_path = generator.generate_stub_file(usage_file, mode="fresh")
    stub_content = stub_path.read_text()

    print("Initial stub:")
    print(stub_content)

    # Verify initial generation has QueryA with one overload and QueryB with generic overload
    assert "class QueryA(SQLRegistry):" in stub_content
    assert "class QueryB(SQLRegistry):" in stub_content
    assert "SELECT id, name FROM users" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content

    # Now add another call for QueryA and one for QueryB
    usage_file.write_text("""
from queries import QueryA, QueryB

query1 = QueryA.sql("SELECT id, name FROM users")
query2 = QueryA.sql("SELECT id, email FROM users")
query3 = QueryB.sql("SELECT id, price FROM products")
""")

    # Generate in incremental mode
    stub_path = generator.generate_stub_file(usage_file, mode="incremental")
    stub_content = stub_path.read_text()

    print("\nIncremental stub:")
    print(stub_content)

    # Verify incremental generation added new overloads
    assert "class QueryA(SQLRegistry):" in stub_content
    assert "class QueryB(SQLRegistry):" in stub_content

    # Should have all three queries
    assert "SELECT id, name FROM users" in stub_content
    assert "SELECT id, email FROM users" in stub_content
    assert "SELECT id, price FROM products" in stub_content

    # Check types are correct
    assert "SQL[tuple[int, str]]" in stub_content  # For name queries
    assert "SQL[tuple[int, float]]" in stub_content  # For price in products


def test_full_mode_with_multiple_registries(tmp_path: Path) -> None:
    """Test that full mode regenerates all overloads for multiple registries."""
    # Create schema files
    schema_a = tmp_path / "schema_a.sql"
    schema_a.write_text("""
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name VARCHAR(100)
        );
    """)

    schema_b = tmp_path / "schema_b.sql"
    schema_b.write_text("""
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            price FLOAT
        );
    """)

    # Create a module with two registries
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""
from sqlty import SQLRegistry

class QueryA(SQLRegistry):
    __schema_path__ = "schema_a.sql"

class QueryB(SQLRegistry):
    __schema_path__ = "schema_b.sql"
""")

    # Create usage file with two queries
    usage_file = tmp_path / "usage.py"
    usage_file.write_text("""
from queries import QueryA, QueryB

query1 = QueryA.sql("SELECT id, name FROM users")
query2 = QueryB.sql("SELECT id, price FROM products")
""")

    generator = TypeStubGenerator()

    # Generate initial stub
    stub_path = generator.generate_stub_file(usage_file, mode="fresh")
    initial_content = stub_path.read_text()

    print("Initial stub:")
    print(initial_content)

    # Manually add a comment to the stub to verify it's preserved
    lines = initial_content.split("\n")
    lines.insert(3, "# Custom comment that should be preserved")
    stub_path.write_text("\n".join(lines))

    # Now remove one query and add a different one
    usage_file.write_text("""
from queries import QueryA

query1 = QueryA.sql("SELECT id, name FROM users")
query2 = QueryA.sql("SELECT name FROM users WHERE id > 10")
""")

    # Generate in full mode
    stub_path = generator.generate_stub_file(usage_file, mode="full")
    full_content = stub_path.read_text()

    print("\nFull mode stub:")
    print(full_content)

    # Verify full mode regenerated classes but preserved custom content
    assert "# Custom comment that should be preserved" in full_content
    assert "class QueryA(SQLRegistry):" in full_content
    assert "class QueryB(SQLRegistry):" in full_content

    # Should have both QueryA queries
    assert "SELECT id, name FROM users" in full_content
    assert "SELECT name FROM users WHERE id > 10" in full_content

    # QueryB should still be there but with minimal stub (no calls)
    # The original call to QueryB should NOT be in the stub since we're in full mode
    # and the new usage doesn't have it
    assert (
        "SELECT id, price FROM products" not in full_content
        or full_content.count("SELECT id, price FROM products") == 0
    )

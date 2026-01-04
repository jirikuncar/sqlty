"""Test for ensuring stable order of overloads in pyi files.

This test ensures that the order of SQL queries in the source file
doesn't affect the order of overloads in the generated .pyi stub file.
The overloads should be sorted deterministically by the SQL query literal.
"""

from pathlib import Path

from sqlty._generator import TypeStubGenerator


def test_stable_overload_order_on_source_reorder(tmp_path: Path) -> None:
    """Test that changing query order in source doesn't affect stub order.

    Test scenario:
    1. Create query file with 2 queries called in one order
    2. Generate pyi stub
    3. Change order of called queries in original file
    4. Regenerate pyi stub and ensure it hasn't changed
    """
    # Step 1: Create source file with queries in alphabetical order (B, then A)
    source_file = tmp_path / "queries.py"
    source_content_order1 = """
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

# Queries called in reverse alphabetical order
query_b = sql("SELECT name::TEXT FROM users WHERE id = :id")
query_a = sql("SELECT id::INTEGER FROM users")
"""
    source_file.write_text(source_content_order1)

    # Step 2: Generate initial pyi stub
    generator = TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query"}]
    )
    stub_file = generator.generate_stub_file(source_file, mode="incremental,create")

    assert stub_file.exists()
    initial_stub_content = stub_file.read_text()

    # Verify both queries are present
    assert "SELECT name::TEXT FROM users WHERE id = :id" in initial_stub_content
    assert "SELECT id::INTEGER FROM users" in initial_stub_content

    # Step 3: Change order of queries in source file (now A, then B)
    source_content_order2 = """
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

# Queries now called in alphabetical order
query_a = sql("SELECT id::INTEGER FROM users")
query_b = sql("SELECT name::TEXT FROM users WHERE id = :id")
"""
    source_file.write_text(source_content_order2)

    # Step 4: Regenerate stub with full mode (complete regeneration)
    stub_file_regenerated = generator.generate_stub_file(source_file, mode="incremental,create")

    assert stub_file_regenerated.exists()
    regenerated_stub_content = stub_file_regenerated.read_text()

    # Step 5: Verify that the stub content hasn't changed
    # The order of overloads should be stable (sorted by literal)
    assert initial_stub_content == regenerated_stub_content, (
        "Stub content changed after reordering queries in source file. "
        "Overloads should be sorted deterministically by SQL literal."
    )


def test_stable_overload_order_with_multiple_queries(tmp_path: Path) -> None:
    """Test stable ordering with more queries to verify consistent sorting.

    Uses 4 queries to ensure the sorting algorithm produces consistent results.
    """
    source_file = tmp_path / "queries.py"

    # Create source with queries in one order (4, 1, 3, 2)
    source_order1 = """
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

# Queries in arbitrary order
d = sql("SELECT updated_at::TIMESTAMP FROM users")
a = sql("SELECT COUNT(*)::INTEGER FROM users")
c = sql("SELECT name::TEXT, email::TEXT FROM users")
b = sql("SELECT id::INTEGER, active::BOOLEAN FROM users")
"""
    source_file.write_text(source_order1)

    # Generate initial stub
    generator = TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query"}]
    )
    stub_file = generator.generate_stub_file(source_file, mode="incremental,create")
    initial_stub = stub_file.read_text()

    # Now create source with queries in different order (2, 3, 4, 1)
    source_order2 = """
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

# Same queries, different order
b = sql("SELECT id::INTEGER, active::BOOLEAN FROM users")
c = sql("SELECT name::TEXT, email::TEXT FROM users")
d = sql("SELECT updated_at::TIMESTAMP FROM users")
a = sql("SELECT COUNT(*)::INTEGER FROM users")
"""
    source_file.write_text(source_order2)

    # Regenerate stub
    stub_file = generator.generate_stub_file(source_file, mode="incremental,create")
    regenerated_stub = stub_file.read_text()

    # Verify stubs are identical despite different source order
    assert initial_stub == regenerated_stub, (
        "Stub content should be identical regardless of query order in source. "
        f"Initial stub length: {len(initial_stub)}, "
        f"Regenerated stub length: {len(regenerated_stub)}"
    )


def test_overload_order_matches_sorted_literals(tmp_path: Path) -> None:
    """Test that overloads appear in alphabetically sorted order by SQL literal."""
    source_file = tmp_path / "queries.py"

    # Create queries with literals that have clear alphabetical order
    source_content = """
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

# Create queries with distinct first characters for easy verification
zebra = sql("zebra query")
apple = sql("apple query")
mango = sql("mango query")
"""
    source_file.write_text(source_content)

    generator = TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query"}]
    )
    stub_file = generator.generate_stub_file(source_file, mode="incremental,create")
    stub_content = stub_file.read_text()

    # Find positions of each query in the stub
    apple_pos = stub_content.find("apple query")
    mango_pos = stub_content.find("mango query")
    zebra_pos = stub_content.find("zebra query")

    # Verify all queries are present
    assert apple_pos != -1, "apple query not found in stub " + stub_content
    assert mango_pos != -1, "mango query not found in stub " + stub_content
    assert zebra_pos != -1, "zebra query not found in stub " + stub_content

    # Verify they appear in alphabetical order
    assert apple_pos < mango_pos < zebra_pos, (
        f"Queries should appear in alphabetical order. "
        f"Positions: apple={apple_pos}, mango={mango_pos}, zebra={zebra_pos}"
    )


def test_stable_order_with_incremental_mode(tmp_path: Path) -> None:
    """Test that incremental mode also maintains stable ordering."""
    source_file = tmp_path / "queries.py"

    # Start with two queries
    source_file.write_text("""
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

query_b = sql("SELECT name::TEXT FROM users")
query_a = sql("SELECT id::INTEGER FROM users")
""")

    generator = TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query"}]
    )
    stub_file = generator.generate_stub_file(source_file, mode="incremental,create")
    initial_stub = stub_file.read_text()
    initial_id_pos = initial_stub.find("SELECT id::INTEGER FROM users")
    initial_name_pos = initial_stub.find("SELECT name::TEXT FROM users")

    assert initial_id_pos != -1
    assert initial_name_pos != -1
    assert initial_id_pos < initial_name_pos, "Initial stub should have id query before name query."

    # Add a third query that should be inserted in the middle alphabetically
    source_file.write_text("""
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

query_b = sql("SELECT name::TEXT FROM users")
query_a = sql("SELECT id::INTEGER FROM users")
query_c = sql("SELECT email::TEXT FROM users")
""")

    # Regenerate with full mode to get consistent ordering
    stub_file = generator.generate_stub_file(source_file, mode="incremental,create")
    final_stub = stub_file.read_text()

    # Find positions
    id_pos = final_stub.find("SELECT id::INTEGER FROM users")
    name_pos = final_stub.find("SELECT name::TEXT FROM users")
    email_pos = final_stub.find("SELECT email::TEXT FROM users")

    # All should be present
    assert id_pos != -1
    assert name_pos != -1
    assert email_pos != -1

    # Email should be inserted between id and name (alphabetically)
    # "SELECT email..." < "SELECT id..." < "SELECT name..."
    assert email_pos < id_pos < name_pos, (
        "Queries should be in alphabetical order by literal. "
        f"Expected: email < id < name, Got positions: "
        f"email={email_pos}, id={id_pos}, name={name_pos}"
    )

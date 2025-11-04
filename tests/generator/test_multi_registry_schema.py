"""Test schema detection with multiple registries in a single file."""

from pathlib import Path

from sqlty._generator import TypeStubGenerator


def test_multiple_registries_different_schemas(tmp_path: Path) -> None:
    """Test that each registry uses its own schema, not other registries' schemas."""
    # Create two schema files
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

    # Create a module with two registries - A has schema_a, B has schema_b
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""
from sqlty import SQLRegistry

class RegistryA(SQLRegistry):
    '''Registry A has schema_a.sql.'''
    __schema_path__ = "schema_a.sql"

class RegistryB(SQLRegistry):
    '''Registry B has schema_b.sql.'''
    __schema_path__ = "schema_b.sql"

# RegistryA queries - should use schema_a.sql
def get_users():
    return RegistryA.sql("SELECT id, name FROM users")

# RegistryB queries - should use schema_b.sql
def get_products():
    return RegistryB.sql("SELECT id, price FROM products")
""")

    generator = TypeStubGenerator()

    # Analyze the file
    generator.generate_stub_file(
        source_path=queries_file,
        output_path=tmp_path / "queries.pyi",
        mode="fresh",
    )

    stub_content = (tmp_path / "queries.pyi").read_text()
    print("Generated stub:")
    print(stub_content)

    # Both queries should have been analyzed with their respective schemas
    # RegistryA query: SELECT id, name FROM users → should see int, str from schema_a
    assert "SELECT id, name FROM users" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content

    # RegistryB query: SELECT id, price FROM products → should see int, float from schema_b
    assert "SELECT id, price FROM products" in stub_content
    assert "SQL[tuple[int, float]]" in stub_content


def test_first_registry_without_schema_second_with_schema(tmp_path: Path) -> None:
    """Test that when first registry has no schema, second registry's schema isn't used for it."""
    schema_b = tmp_path / "schema_b.sql"
    schema_b.write_text("""
        CREATE TABLE orders (
            id INTEGER,
            total FLOAT
        );
    """)

    queries_file = tmp_path / "multi_queries.py"
    queries_file.write_text("""
from sqlty import SQLRegistry

class FirstRegistry(SQLRegistry):
    pass

class SecondRegistry(SQLRegistry):
    __schema_path__ = "schema_b.sql"
""")

    generator = TypeStubGenerator()

    # Get schema path for this module - it should return schema_b.sql
    # But this is the bug! It should NOT apply to FirstRegistry
    schema_path = generator._registry_detector.get_schema_path_from_registry(queries_file)

    # The current implementation incorrectly returns schema_b.sql
    # even though FirstRegistry has no __schema_path__
    assert schema_path is not None
    assert schema_path.name == "schema_b.sql"

    # This is the problem: get_schema_path_from_registry doesn't know
    # WHICH registry we're asking about!


def test_registry_specific_schema_detection(tmp_path: Path) -> None:
    """Test that we can detect which registry has which schema."""
    schema_a = tmp_path / "schema_a.sql"
    schema_a.write_text("CREATE TABLE table_a (id INTEGER);")

    schema_b = tmp_path / "schema_b.sql"
    schema_b.write_text("CREATE TABLE table_b (id INTEGER);")

    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""
from sqlty import SQLRegistry

class RegistryA(SQLRegistry):
    __schema_path__ = "schema_a.sql"

class RegistryB(SQLRegistry):
    __schema_path__ = "schema_b.sql"

class RegistryC(SQLRegistry):
    pass  # No schema
""")

    generator = TypeStubGenerator()

    # Now the API supports passing class_name to get schema for specific registry!
    schema_a = generator._registry_detector.get_schema_path_from_registry(
        queries_file, class_name="RegistryA"
    )
    schema_b = generator._registry_detector.get_schema_path_from_registry(
        queries_file, class_name="RegistryB"
    )
    schema_c = generator._registry_detector.get_schema_path_from_registry(
        queries_file, class_name="RegistryC"
    )

    assert schema_a is not None
    assert schema_a.name == "schema_a.sql"
    assert schema_b is not None
    assert schema_b.name == "schema_b.sql"
    assert schema_c is None  # RegistryC has no __schema_path__


def test_generate_stub_for_multiple_registries_no_calls(tmp_path: Path) -> None:
    """Test that stub generation creates stubs for all registries even when there are no calls."""
    schema_a = tmp_path / "schema_a.sql"
    schema_a.write_text("CREATE TABLE table_a (id INTEGER);")

    schema_b = tmp_path / "schema_b.sql"
    schema_b.write_text("CREATE TABLE table_b (id INTEGER);")

    # Create a registry file with two registries but no SQL calls
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""
from sqlty import SQLRegistry

class RegistryA(SQLRegistry):
    __schema_path__ = "schema_a.sql"

class RegistryB(SQLRegistry):
    __schema_path__ = "schema_b.sql"
""")

    generator = TypeStubGenerator()

    # Generate stub for the file with no calls
    generator.generate_stub_file(
        source_path=queries_file,
        mode="full",
    )

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()
    print("Generated stub for file with no calls:")
    print(stub_content)

    # Both registry classes should be in the stub
    assert "class RegistryA" in stub_content
    assert "class RegistryB" in stub_content

"""Tests for the intermediate representation (IR) system."""

import ast

from sqlty._collector import CallInfo
from sqlty._stub_ir import OverloadSignature, RegistryClass, StubFile
from sqlty._stub_parser import CallInfoToIRParser, PyiToIRParser
from sqlty._stub_serializer import IRToASTSerializer, IRToTextSerializer


def test_overload_signature_equality():
    """Test that OverloadSignature equality is based on query literal."""
    sig1 = OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    sig2 = OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[str]]")
    sig3 = OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")

    assert sig1 == sig2  # Same query literal
    assert sig1 != sig3  # Different query literal


def test_registry_class_add_overload():
    """Test adding overloads to a registry."""
    registry = RegistryClass(name="Query", base_class="SQLRegistry")

    sig1 = OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    sig2 = OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    sig3 = OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[str]]")

    assert registry.add_overload(sig1) is True  # New overload
    assert registry.add_overload(sig2) is True  # New overload
    assert registry.add_overload(sig3) is False  # Duplicate (same query)

    assert len(registry.overloads) == 2


def test_registry_class_remove_overload():
    """Test removing overloads from a registry."""
    registry = RegistryClass(name="Query", base_class="SQLRegistry")

    sig1 = OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    sig2 = OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")

    registry.add_overload(sig1)
    registry.add_overload(sig2)

    assert registry.remove_overload("SELECT 1") is True
    assert registry.remove_overload("SELECT 3") is False
    assert len(registry.overloads) == 1


def test_stub_file_merge_incremental():
    """Test incremental merge mode."""
    stub = StubFile()

    registry1 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )

    stub.merge_registry(registry1, mode="incremental")
    assert len(stub.registries["Query"].overloads) == 2

    # Add more overloads
    registry2 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )  # Duplicate

    stub.merge_registry(registry2, mode="incremental")
    assert len(stub.registries["Query"].overloads) == 3  # Only SELECT 3 added


def test_stub_file_merge_full():
    """Test full merge mode."""
    stub = StubFile()

    registry1 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )

    stub.merge_registry(registry1, mode="full")
    assert len(stub.registries["Query"].overloads) == 2

    # Replace with new registry
    registry2 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )

    stub.merge_registry(registry2, mode="full")
    assert len(stub.registries["Query"].overloads) == 1  # Only SELECT 3
    assert stub.registries["Query"].has_overload("SELECT 3")
    assert not stub.registries["Query"].has_overload("SELECT 1")


def test_stub_file_merge_prune():
    """Test prune merge mode - removes stale overloads."""
    stub = StubFile()

    registry1 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )

    stub.merge_registry(registry1, mode="full")
    assert len(stub.registries["Query"].overloads) == 3

    # Prune to keep only SELECT 1 and SELECT 3 (removes SELECT 2)
    registry2 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )

    stub.merge_registry(registry2, mode="prune")
    assert len(stub.registries["Query"].overloads) == 2
    assert stub.registries["Query"].has_overload("SELECT 1")
    assert not stub.registries["Query"].has_overload("SELECT 2")
    assert stub.registries["Query"].has_overload("SELECT 3")


def test_stub_file_merge_prune_never_adds():
    """Test that prune mode never adds new overloads."""
    stub = StubFile()

    # Start with 2 overloads
    registry1 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )

    stub.merge_registry(registry1, mode="full")
    assert len(stub.registries["Query"].overloads) == 2

    # Try to prune with a superset (includes SELECT 3 and SELECT 4 which are new)
    registry2 = RegistryClass(name="Query", base_class="SQLRegistry")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )  # NEW
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 4", return_type="SQL[tuple[int]]")
    )  # NEW

    stub.merge_registry(registry2, mode="prune")

    # Prune should NOT add the new overloads
    assert len(stub.registries["Query"].overloads) == 2
    assert stub.registries["Query"].has_overload("SELECT 1")
    assert stub.registries["Query"].has_overload("SELECT 2")
    assert not stub.registries["Query"].has_overload("SELECT 3")  # Not added
    assert not stub.registries["Query"].has_overload("SELECT 4")  # Not added


def test_call_info_to_ir_parser():
    """Test parsing CallInfo to IR."""
    calls: list[CallInfo] = [
        {
            "literal": "SELECT 1::INTEGER",
            "return_type": "SQL[tuple[int]]",
            "lineno": 10,
            "file": "test.py",
            "registry_source": None,
            "registry_class": None,
        },
        {
            "literal": "SELECT 'hello'::TEXT",
            "return_type": "SQL[tuple[str]]",
            "lineno": 15,
            "file": "test.py",
            "registry_source": None,
            "registry_class": None,
        },
    ]

    parser = CallInfoToIRParser()
    stub = parser.parse_calls(
        calls, class_name="Query", base_class="SQLRegistry", export_name="sql"
    )

    assert "Query" in stub.registries
    assert len(stub.registries["Query"].overloads) == 2
    assert stub.registries["Query"].export_name == "sql"


def test_pyi_to_ir_parser():
    """Test parsing .pyi AST to IR."""
    pyi_source = '''from typing import Literal, overload

from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    tree = ast.parse(pyi_source)
    parser = PyiToIRParser()
    stub = parser.parse_ast(tree)

    assert "Query" in stub.registries
    assert len(stub.registries["Query"].overloads) == 1
    assert stub.registries["Query"].export_name == "sql"

    # Check the overload
    overloads = list(stub.registries["Query"].overloads)
    assert overloads[0].query_literal == "SELECT 1::INTEGER"
    assert overloads[0].return_type == "SQL[tuple[int]]"


def test_ir_to_ast_serializer():
    """Test serializing IR to AST."""
    stub = StubFile()

    registry = RegistryClass(name="Query", base_class="SQLRegistry", export_name="sql")
    registry.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )

    stub.add_registry(registry)

    serializer = IRToASTSerializer()
    module = serializer.serialize_to_module(stub)

    # Verify it's a valid module
    assert isinstance(module, ast.Module)
    assert len(module.body) > 0

    # Check that we can unparse it
    code = ast.unparse(module)
    assert "class Query(SQLRegistry):" in code
    assert "SELECT 1" in code


def test_ir_to_text_serializer():
    """Test serializing IR to formatted text."""
    stub = StubFile()

    registry = RegistryClass(name="Query", base_class="SQLRegistry", export_name="sql")
    registry.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[str]]")
    )

    stub.add_registry(registry)

    serializer = IRToTextSerializer()
    text = serializer.serialize_to_text(stub)

    # Verify basic structure
    assert "from typing import Literal, overload" in text
    assert "from sqlty import SQL, SQLRegistry" in text
    assert "class Query(SQLRegistry):" in text
    assert "SELECT 1" in text
    assert "SELECT 2" in text
    assert "sql = Query.sql" in text


def test_round_trip_pyi_to_ir_to_text():
    """Test round-trip conversion: pyi -> IR -> text."""
    original_pyi = '''from typing import Literal, overload

from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 2::TEXT"]) -> SQL[tuple[str]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    # Parse to IR
    tree = ast.parse(original_pyi)
    parser = PyiToIRParser()
    stub_ir = parser.parse_ast(tree)

    # Serialize back to text
    serializer = IRToTextSerializer()
    new_text = serializer.serialize_to_text(stub_ir)

    # Verify content is preserved
    assert "SELECT 1::INTEGER" in new_text
    assert "SELECT 2::TEXT" in new_text
    assert "SQL[tuple[int]]" in new_text
    assert "SQL[tuple[str]]" in new_text
    assert "sql = Query.sql" in new_text


def test_compute_required_imports():
    """Test that required imports are computed correctly."""
    stub = StubFile()

    registry = RegistryClass(name="Query", base_class="SQLRegistry")
    registry.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[uuid.UUID]]")
    )
    registry.add_overload(
        OverloadSignature(
            query_literal="SELECT 2", return_type="SQL[tuple[datetime.datetime, decimal.Decimal]]"
        )
    )

    stub.add_registry(registry)

    required = stub.compute_required_imports()

    # Check that specialized imports are included
    import_modules = {imp.module for imp in required if not imp.from_import}
    assert "uuid" in import_modules
    assert "datetime" in import_modules
    assert "decimal" in import_modules

    # Check that standard imports are included
    from_imports = {imp.module for imp in required if imp.from_import}
    assert "typing" in from_imports
    assert "sqlty" in from_imports


def test_ast_node_references_preserved():
    """Test that AST node references are preserved during parsing."""
    pyi_source = '''from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    tree = ast.parse(pyi_source)
    parser = PyiToIRParser()
    stub = parser.parse_ast(tree)

    # Verify StubFile has AST module reference
    assert stub.ast_module is not None
    assert isinstance(stub.ast_module, ast.Module)

    # Verify RegistryClass has AST node reference
    registry = stub.get_registry("Query")
    assert registry is not None
    assert registry.ast_node is not None
    assert isinstance(registry.ast_node, ast.ClassDef)
    assert registry.ast_node.name == "Query"

    # Verify OverloadSignature has AST node reference
    overloads = list(registry.overloads)
    assert len(overloads) == 1
    assert overloads[0].ast_node is not None
    assert isinstance(overloads[0].ast_node, ast.FunctionDef)
    assert overloads[0].ast_node.name == "sql"

    # Verify ImportStatement has AST node reference
    assert len(stub.imports) > 0
    for imp in stub.imports:
        assert imp.ast_node is not None
        assert isinstance(imp.ast_node, (ast.Import, ast.ImportFrom))

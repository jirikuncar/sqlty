"""Tests for the intermediate representation (IR) system."""

import ast

import asttokens

from sqlty._collector import CallInfo
from sqlty._stub_ir import OverloadSignature, RegistryDecorator, StubFile
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
    registry = RegistryDecorator(name="Query", base_class="SQLRegistry")

    sig1 = OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    sig2 = OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    sig3 = OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[str]]")

    assert registry.add_overload(sig1) is True  # New overload
    assert registry.add_overload(sig2) is True  # New overload
    assert registry.add_overload(sig3) is False  # Duplicate (same query)

    assert len(registry.overloads) == 2


def test_registry_class_remove_overload():
    """Test removing overloads from a registry."""
    registry = RegistryDecorator(name="Query", base_class="SQLRegistry")

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

    registry1 = RegistryDecorator(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )

    stub.merge_registry(registry1, mode="incremental")
    assert len(stub.registries["Query"].overloads) == 2

    # Add more overloads
    registry2 = RegistryDecorator(name="Query", base_class="SQLRegistry")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )  # Duplicate

    stub.merge_registry(registry2, mode="incremental")
    assert len(stub.registries["Query"].overloads) == 3  # Only SELECT 3 added


def test_stub_file_replace_via_add_registry():
    """Test replacing entire registry via add_registry (no merge mode)."""
    stub = StubFile()

    registry1 = RegistryDecorator(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )

    stub.add_registry(registry1)
    assert len(stub.registries["Query"].overloads) == 2

    # Replace with new registry
    registry2 = RegistryDecorator(name="Query", base_class="SQLRegistry")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )

    stub.add_registry(registry2)
    assert len(stub.registries["Query"].overloads) == 1  # Only SELECT 3
    assert stub.registries["Query"].has_overload("SELECT 3")
    assert not stub.registries["Query"].has_overload("SELECT 1")


def test_stub_file_merge_prune():
    """Test prune merge mode - removes stale overloads."""
    stub = StubFile()

    registry1 = RegistryDecorator(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[int]]")
    )

    stub.add_registry(registry1)
    assert len(stub.registries["Query"].overloads) == 3

    # Prune to keep only SELECT 1 and SELECT 3 (removes SELECT 2)
    registry2 = RegistryDecorator(name="Query", base_class="SQLRegistry")
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
    registry1 = RegistryDecorator(name="Query", base_class="SQLRegistry")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[int]]")
    )

    stub.add_registry(registry1)
    assert len(stub.registries["Query"].overloads) == 2

    # Try to prune with a superset (includes SELECT 3 and SELECT 4 which are new)
    registry2 = RegistryDecorator(name="Query", base_class="SQLRegistry")
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

    registry = RegistryDecorator(name="Query", base_class="SQLRegistry", export_name="sql")
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

    registry = RegistryDecorator(name="Query", base_class="SQLRegistry", export_name="sql")
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

    registry = RegistryDecorator(name="Query", base_class="SQLRegistry")
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

    # Verify RegistryDecorator has AST node reference
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


def test_duplicate_alias_omission():
    """Test that duplicate export aliases are omitted from serialized output."""
    stub = StubFile()

    # Create two registries with the same export name
    registry1 = RegistryDecorator(name="QueryA", base_class="SQLRegistry", export_name="sql")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )

    registry2 = RegistryDecorator(name="QueryB", base_class="SQLRegistry", export_name="sql")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[str]]")
    )

    stub.add_registry(registry1)
    stub.add_registry(registry2)

    # Serialize to text
    serializer = IRToTextSerializer()
    text = serializer.serialize_to_text(stub)

    # Both classes should be present
    assert "class QueryA(SQLRegistry):" in text
    assert "class QueryB(SQLRegistry):" in text

    # No duplicate aliases should be present
    # Count how many times "sql = " appears (should be 0 because export name is duplicated)
    alias_count = text.count("sql = ")
    assert alias_count == 0, f"Expected 0 aliases but found {alias_count}"


def test_unique_alias_inclusion():
    """Test that unique export aliases are included in serialized output."""
    stub = StubFile()

    # Create registries with unique export names
    registry1 = RegistryDecorator(name="UserQuery", base_class="SQLRegistry", export_name="user_sql")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT * FROM users", return_type="SQL[tuple[int, str]]")
    )

    registry2 = RegistryDecorator(name="PostQuery", base_class="SQLRegistry", export_name="post_sql")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT * FROM posts", return_type="SQL[tuple[int, str]]")
    )

    stub.add_registry(registry1)
    stub.add_registry(registry2)

    # Serialize to text
    serializer = IRToTextSerializer()
    text = serializer.serialize_to_text(stub)

    # Both classes should be present
    assert "class UserQuery(SQLRegistry):" in text
    assert "class PostQuery(SQLRegistry):" in text

    # Both unique aliases should be present
    assert "user_sql = UserQuery.sql" in text
    assert "post_sql = PostQuery.sql" in text


def test_mixed_alias_scenario():
    """Test scenario with both unique and duplicate export names."""
    stub = StubFile()

    # Two registries with the same export name
    registry1 = RegistryDecorator(name="QueryA", base_class="SQLRegistry", export_name="sql")
    registry1.add_overload(
        OverloadSignature(query_literal="SELECT 1", return_type="SQL[tuple[int]]")
    )

    registry2 = RegistryDecorator(name="QueryB", base_class="SQLRegistry", export_name="sql")
    registry2.add_overload(
        OverloadSignature(query_literal="SELECT 2", return_type="SQL[tuple[str]]")
    )

    # One registry with a unique export name
    registry3 = RegistryDecorator(
        name="SpecialQuery", base_class="SQLRegistry", export_name="special_sql"
    )
    registry3.add_overload(
        OverloadSignature(query_literal="SELECT 3", return_type="SQL[tuple[float]]")
    )

    stub.add_registry(registry1)
    stub.add_registry(registry2)
    stub.add_registry(registry3)

    # Serialize to text
    serializer = IRToTextSerializer()
    text = serializer.serialize_to_text(stub)

    # All classes should be present
    assert "class QueryA(SQLRegistry):" in text
    assert "class QueryB(SQLRegistry):" in text
    assert "class SpecialQuery(SQLRegistry):" in text

    # Duplicate aliases should NOT be present
    assert "sql = QueryA.sql" not in text
    assert "sql = QueryB.sql" not in text

    # Unique alias should be present
    assert "special_sql = SpecialQuery.sql" in text


def test_source_span_tracking_in_pyi_parser():
    """Test that source spans are tracked when parsing .pyi files."""
    pyi_source = '''from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 'hello'::TEXT"]) -> SQL[tuple[str]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    tree = ast.parse(pyi_source)
    parser = PyiToIRParser()
    # Set up ASTTokens for source span extraction
    parser.atok = asttokens.ASTTokens(pyi_source, tree=tree)
    stub = parser.parse_ast(tree)

    # Verify overloads have source spans
    registry = stub.get_registry("Query")
    assert registry is not None

    overloads = list(registry.overloads)
    assert len(overloads) == 2

    # Check that both overloads have source spans
    for overload in overloads:
        assert overload.source_span is not None
        assert overload.source_span.start_offset >= 0
        assert overload.source_span.end_offset > overload.source_span.start_offset
        assert overload.source_span.start_line > 0
        assert overload.source_span.end_line >= overload.source_span.start_line
        assert overload.source_span.start_col >= 0
        assert overload.source_span.end_col >= 0


def test_source_span_tracking_from_call_info():
    """Test that source spans are propagated from CallInfo to IR."""
    calls: list[CallInfo] = [
        {
            "literal": "SELECT 1::INTEGER",
            "return_type": "SQL[tuple[int]]",
            "lineno": 10,
            "file": "test.py",
            "registry_source": None,
            "registry_class": None,
            "start_offset": 100,
            "end_offset": 150,
            "start_col": 5,
            "end_line": 10,
            "end_col": 55,
        },
        {
            "literal": "SELECT 'hello'::TEXT",
            "return_type": "SQL[tuple[str]]",
            "lineno": 15,
            "file": "test.py",
            "registry_source": None,
            "registry_class": None,
            "start_offset": 200,
            "end_offset": 250,
            "start_col": 5,
            "end_line": 15,
            "end_col": 55,
        },
    ]

    parser = CallInfoToIRParser()
    stub = parser.parse_calls(
        calls, class_name="Query", base_class="SQLRegistry", export_name="sql"
    )

    registry = stub.get_registry("Query")
    assert registry is not None

    overloads = list(registry.overloads)
    assert len(overloads) == 2

    # Check that source spans are preserved
    for overload in overloads:
        assert overload.source_span is not None
        assert overload.source_span.start_offset in [100, 200]
        assert overload.source_span.end_offset in [150, 250]


def test_source_span_stability_across_edits():
    """Test that source spans remain stable when unrelated parts are edited."""
    original_pyi = '''from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 2::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    # Parse original
    tree = ast.parse(original_pyi)
    parser = PyiToIRParser()
    # Set up ASTTokens for source span extraction
    parser.atok = asttokens.ASTTokens(original_pyi, tree=tree)
    stub = parser.parse_ast(tree)

    registry = stub.get_registry("Query")
    assert registry is not None

    original_overloads = {o.query_literal: o.source_span for o in registry.overloads}

    # Simulate editing: add a new overload to the source
    edited_pyi = original_pyi.replace(
        "    def sql[T](cls, query: T) -> T: ...",
        """    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 3::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...""",
    )

    # Parse edited version
    tree2 = ast.parse(edited_pyi)
    parser2 = PyiToIRParser()
    parser2.atok = asttokens.ASTTokens(edited_pyi, tree=tree2)
    stub2 = parser2.parse_ast(tree2)

    registry2 = stub2.get_registry("Query")
    assert registry2 is not None

    # Original overloads should have similar (but not necessarily identical) positions
    # since we added text before the fallback
    for overload in registry2.overloads:
        if overload.query_literal in original_overloads:
            original_span = original_overloads[overload.query_literal]
            # Line numbers should be same for the first two overloads
            # (they come before the insertion point)
            if overload.query_literal in ["SELECT 1::INTEGER", "SELECT 2::INTEGER"]:
                assert original_span is not None
                assert overload.source_span is not None
                assert overload.source_span.start_line == original_span.start_line


def test_incremental_mode_preserves_existing_overloads():
    """Test that incremental mode only adds new overloads without modifying existing ones."""
    # Parse existing stub
    existing_pyi = '''from typing import Literal, overload
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

    tree = ast.parse(existing_pyi)
    parser = PyiToIRParser()
    existing_stub = parser.parse_ast(tree)

    # Verify initial state
    registry = existing_stub.get_registry("Query")
    assert registry is not None
    assert len(registry.overloads) == 1
    assert registry.has_overload("SELECT 1::INTEGER")

    # Create new calls to add
    new_calls: list[CallInfo] = [
        {
            "literal": "SELECT 2::INTEGER",
            "return_type": "SQL[tuple[int]]",
            "lineno": 20,
            "file": "test.py",
            "registry_source": None,
            "registry_class": None,
            "start_offset": None,
            "end_offset": None,
            "start_col": None,
            "end_line": None,
            "end_col": None,
        },
    ]

    call_parser = CallInfoToIRParser()
    new_stub = call_parser.parse_calls(
        new_calls, class_name="Query", base_class="SQLRegistry", export_name="sql"
    )

    # Merge in incremental mode
    for new_registry in new_stub.registries.values():
        existing_stub.merge_registry(new_registry, mode="incremental")

    # Verify that both old and new overloads exist
    registry = existing_stub.get_registry("Query")
    assert registry is not None
    assert len(registry.overloads) == 2
    assert registry.has_overload("SELECT 1::INTEGER")  # Preserved
    assert registry.has_overload("SELECT 2::INTEGER")  # Added


def test_prune_mode_preserves_existing_overloads():
    """Test that prune mode only removes stale overloads without modifying kept ones."""
    # Parse existing stub with 3 overloads
    existing_pyi = '''from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 2::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 3::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    tree = ast.parse(existing_pyi)
    parser = PyiToIRParser()
    existing_stub = parser.parse_ast(tree)

    # Verify initial state
    registry = existing_stub.get_registry("Query")
    assert registry is not None
    assert len(registry.overloads) == 3

    # Create new calls representing current state (only SELECT 1 and SELECT 3)
    new_calls: list[CallInfo] = [
        {
            "literal": "SELECT 1::INTEGER",
            "return_type": "SQL[tuple[int]]",
            "lineno": 10,
            "file": "test.py",
            "registry_source": None,
            "registry_class": None,
            "start_offset": None,
            "end_offset": None,
            "start_col": None,
            "end_line": None,
            "end_col": None,
        },
        {
            "literal": "SELECT 3::INTEGER",
            "return_type": "SQL[tuple[int]]",
            "lineno": 20,
            "file": "test.py",
            "registry_source": None,
            "registry_class": None,
            "start_offset": None,
            "end_offset": None,
            "start_col": None,
            "end_line": None,
            "end_col": None,
        },
    ]

    call_parser = CallInfoToIRParser()
    new_stub = call_parser.parse_calls(
        new_calls, class_name="Query", base_class="SQLRegistry", export_name="sql"
    )

    # Merge in prune mode
    for new_registry in new_stub.registries.values():
        existing_stub.merge_registry(new_registry, mode="prune")

    # Verify that SELECT 2 was removed, but others were preserved
    registry = existing_stub.get_registry("Query")
    assert registry is not None
    assert len(registry.overloads) == 2
    assert registry.has_overload("SELECT 1::INTEGER")  # Preserved
    assert not registry.has_overload("SELECT 2::INTEGER")  # Removed
    assert registry.has_overload("SELECT 3::INTEGER")  # Preserved


def test_surgical_editing_preserves_comments():
    """Test that surgical editing preserves comments when adding/removing overloads."""
    import tempfile
    from pathlib import Path

    # Create a temporary stub file with comments
    stub_content = '''from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    # This is a comment about query 1
    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    # This is a comment about query 2
    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 2::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    with tempfile.TemporaryDirectory() as tmpdir:
        stub_path = Path(tmpdir) / "queries.pyi"
        stub_path.write_text(stub_content)

        # Parse the existing stub
        from sqlty._stub_parser import CallInfoToIRParser, PyiToIRParser
        from sqlty._stub_serializer import IRToTextSerializer

        parser = PyiToIRParser()
        existing_stub = parser.parse_file(stub_path)

        # Create new calls that:
        # 1. Keep SELECT 1
        # 2. Remove SELECT 2
        # 3. Add SELECT 3
        new_calls: list[CallInfo] = [
            {
                "literal": "SELECT 1::INTEGER",
                "return_type": "SQL[tuple[int]]",
                "lineno": 10,
                "file": "test.py",
                "registry_source": None,
                "registry_class": None,
                "start_offset": None,
                "end_offset": None,
                "start_col": None,
                "end_line": None,
                "end_col": None,
            },
            {
                "literal": "SELECT 3::INTEGER",
                "return_type": "SQL[tuple[int]]",
                "lineno": 20,
                "file": "test.py",
                "registry_source": None,
                "registry_class": None,
                "start_offset": None,
                "end_offset": None,
                "start_col": None,
                "end_line": None,
                "end_col": None,
            },
        ]

        call_parser = CallInfoToIRParser()
        new_stub = call_parser.parse_calls(
            new_calls, class_name="Query", base_class="SQLRegistry", export_name="sql"
        )

        # Synchronize by applying incremental then prune
        for new_registry in new_stub.registries.values():
            # Add new overloads
            existing_stub.merge_registry(new_registry, mode="incremental")
            # Remove stale ones
            existing_stub.merge_registry(new_registry, mode="prune")

        # Re-parse to get original stub with source spans for comparison
        original_stub = parser.parse_file(stub_path)

        # Serialize with structure preservation
        serializer = IRToTextSerializer()
        result = serializer.serialize_to_text(
            existing_stub,
            preserve_structure=True,
            original_source=stub_content,
            original_stub=original_stub,
        )

        # Verify the results
        assert "SELECT 1::INTEGER" in result
        assert "SELECT 3::INTEGER" in result
        assert "SELECT 2::INTEGER" not in result

        # Most importantly - verify the comment about query 1 is preserved!
        assert "# This is a comment about query 1" in result

        # The comment about query 2 should be gone (with the overload)
        # This is expected behavior when removing overloads with their decorators


def test_surgical_editing_incremental_mode_preserves_structure():
    """Test that incremental mode preserves existing overloads and structure."""
    import tempfile
    from pathlib import Path

    # Create a temporary stub file
    stub_content = '''from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1::INTEGER"]) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
'''

    with tempfile.TemporaryDirectory() as tmpdir:
        stub_path = Path(tmpdir) / "queries.pyi"
        stub_path.write_text(stub_content)

        from sqlty._stub_parser import CallInfoToIRParser, PyiToIRParser
        from sqlty._stub_serializer import IRToTextSerializer

        parser = PyiToIRParser()
        existing_stub = parser.parse_file(stub_path)

        # Add a new overload
        new_calls: list[CallInfo] = [
            {
                "literal": "SELECT 2::INTEGER",
                "return_type": "SQL[tuple[int]]",
                "lineno": 20,
                "file": "test.py",
                "registry_source": None,
                "registry_class": None,
                "start_offset": None,
                "end_offset": None,
                "start_col": None,
                "end_line": None,
                "end_col": None,
            },
        ]

        call_parser = CallInfoToIRParser()
        new_stub = call_parser.parse_calls(
            new_calls, class_name="Query", base_class="SQLRegistry", export_name="sql"
        )

        # Merge in incremental mode
        for new_registry in new_stub.registries.values():
            existing_stub.merge_registry(new_registry, mode="incremental")

        # Re-parse to get original stub with source spans for comparison
        original_stub = parser.parse_file(stub_path)

        # Serialize with structure preservation
        serializer = IRToTextSerializer()
        result = serializer.serialize_to_text(
            existing_stub,
            preserve_structure=True,
            original_source=stub_content,
            original_stub=original_stub,
        )

        # Verify both overloads are present
        assert "SELECT 1::INTEGER" in result
        assert "SELECT 2::INTEGER" in result

        # The docstring should be preserved
        assert "Application-specific SQL query registry." in result


def test_blank_line_removal_logic():
    """Test that _find_removal_end_offset properly skips multiple consecutive blank lines."""
    from sqlty._stub_serializer import IRToTextSerializer

    serializer = IRToTextSerializer()

    # Test case 1: Single blank line after content
    source1 = "content\n\nmore content"
    end_offset1 = 8  # After "content\n"
    result1 = serializer._find_removal_end_offset(source1, end_offset1)
    assert result1 == 9, f"Expected 9, got {result1} - should skip one blank line"

    # Test case 2: Multiple consecutive blank lines
    source2 = "content\n\n\n\nmore content"
    end_offset2 = 8  # After "content\n"
    result2 = serializer._find_removal_end_offset(source2, end_offset2)
    assert result2 == 11, f"Expected 11, got {result2} - should skip three blank lines"

    # Test case 3: No blank lines after content
    source3 = "content\nmore content"
    end_offset3 = 8  # After "content\n"
    result3 = serializer._find_removal_end_offset(source3, end_offset3)
    assert result3 == 8, f"Expected 8, got {result3} - should not skip when next line has content"

    # Test case 4: At end of file with blank lines
    source4 = "content\n\n\n"
    end_offset4 = 8  # After "content\n"
    result4 = serializer._find_removal_end_offset(source4, end_offset4)
    assert result4 == 10, f"Expected 10, got {result4} - should skip all trailing blank lines"

    # Test case 5: Already at end of file
    source5 = "content"
    end_offset5 = 7  # At end
    result5 = serializer._find_removal_end_offset(source5, end_offset5)
    assert result5 == 7, f"Expected 7, got {result5} - should not move past end of file"

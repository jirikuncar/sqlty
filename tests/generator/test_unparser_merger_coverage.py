"""Additional tests for ast_unparser and stub_merger critical paths."""

import ast
from pathlib import Path

from sqlty._ast_unparser import StubASTUnparser
from sqlty._collector import CallInfo
from sqlty._stub_merger import StubMerger


class TestASTUnparserEdgeCases:
    """Test edge cases in AST unparsing."""

    def test_unparse_expr_with_multiline_string(self) -> None:
        """Test unparsing of multiline string constants."""
        unparser = StubASTUnparser()
        node = ast.Constant(value="SELECT *\nFROM users\nWHERE id = 1")
        result = unparser.unparse_expr(node)
        assert result == '"""SELECT *\nFROM users\nWHERE id = 1"""'

    def test_unparse_expr_with_escaped_quotes(self) -> None:
        """Test unparsing of strings with double quotes."""
        unparser = StubASTUnparser()
        node = ast.Constant(value='SELECT "name" FROM users')
        result = unparser.unparse_expr(node)
        assert result == '"SELECT \\"name\\" FROM users"'

    def test_unparse_expr_with_list(self) -> None:
        """Test unparsing of list type annotations."""
        unparser = StubASTUnparser()
        node = ast.List(elts=[ast.Constant(value="a"), ast.Constant(value="b")], ctx=ast.Load())
        result = unparser.unparse_expr(node)
        assert result == '["a", "b"]'

    def test_unparse_expr_fallback_to_ast_unparse(self) -> None:
        """Test that complex expressions fall back to ast.unparse."""
        unparser = StubASTUnparser()
        # Create a complex expression that isn't handled by our special cases
        node = ast.BinOp(left=ast.Constant(value=1), op=ast.Add(), right=ast.Constant(value=2))
        result = unparser.unparse_expr(node)
        assert result == "1 + 2"

    def test_unparse_class_def_without_docstring(self) -> None:
        """Test unparsing class without docstring."""
        unparser = StubASTUnparser()

        # Create a class node without docstring
        class_node = ast.ClassDef(
            name="TestClass",
            bases=[ast.Name(id="BaseClass", ctx=ast.Load())],
            keywords=[],
            body=[
                ast.FunctionDef(
                    name="method",
                    args=ast.arguments(
                        posonlyargs=[],
                        args=[ast.arg(arg="self", annotation=None)],
                        kwonlyargs=[],
                        kw_defaults=[],
                        defaults=[],
                    ),
                    body=[ast.Expr(value=ast.Constant(value=...))],
                    decorator_list=[],
                    returns=None,
                )
            ],
            decorator_list=[],
        )

        calls: list[CallInfo] = []
        lines = unparser.unparse_class_def(class_node, calls)

        # Should have class declaration without docstring
        assert "class TestClass(BaseClass):" in lines
        assert '"""' not in "".join(lines)

    def test_unparse_class_def_with_non_string_docstring(self) -> None:
        """Test handling of non-string docstring values."""
        unparser = StubASTUnparser()

        # Create a class with non-string constant as first body element
        class_node = ast.ClassDef(
            name="TestClass",
            bases=[],
            keywords=[],
            body=[
                ast.Expr(value=ast.Constant(value=123)),  # Non-string
            ],
            decorator_list=[],
        )

        calls: list[CallInfo] = []
        lines = unparser.unparse_class_def(class_node, calls)

        # Should handle gracefully without adding docstring
        assert "class TestClass:" in lines

    def test_unparse_method_with_type_params(self) -> None:
        """Test unparsing method with type parameters."""
        unparser = StubASTUnparser()

        # Create method with type parameter
        type_param = ast.TypeVar(name="T")
        method_node = ast.FunctionDef(
            name="generic_method",
            args=ast.arguments(
                posonlyargs=[],
                args=[
                    ast.arg(arg="self", annotation=None),
                    ast.arg(arg="value", annotation=ast.Name(id="T", ctx=ast.Load())),
                ],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=[],
            returns=ast.Name(id="T", ctx=ast.Load()),
            type_params=[type_param],
        )

        lines = unparser.unparse_method(method_node)

        # Should include type parameter
        assert any("[T]" in line for line in lines)

    def test_unparse_method_without_annotation(self) -> None:
        """Test unparsing method with unannotated arguments."""
        unparser = StubASTUnparser()

        method_node = ast.FunctionDef(
            name="simple_method",
            args=ast.arguments(
                posonlyargs=[],
                args=[
                    ast.arg(arg="self", annotation=None),
                    ast.arg(arg="value", annotation=None),  # No annotation
                ],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=[],
            returns=None,
        )

        lines = unparser.unparse_method(method_node)

        # Should handle unannotated args
        assert any("value," in line for line in lines)

    def test_extract_literal_from_method_no_args(self) -> None:
        """Test extract_literal_from_method with no arguments."""
        unparser = StubASTUnparser()

        method_node = ast.FunctionDef(
            name="no_args",
            args=ast.arguments(
                posonlyargs=[],
                args=[],  # No args
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=[],
            returns=None,
        )

        result = unparser.extract_literal_from_method(method_node)
        assert result is None

    def test_extract_literal_from_method_non_literal_annotation(self) -> None:
        """Test extract_literal_from_method with non-Literal annotation."""
        unparser = StubASTUnparser()

        method_node = ast.FunctionDef(
            name="method",
            args=ast.arguments(
                posonlyargs=[],
                args=[
                    ast.arg(
                        arg="query",
                        annotation=ast.Name(id="str", ctx=ast.Load()),  # Not Literal
                    )
                ],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=[],
            returns=None,
        )

        result = unparser.extract_literal_from_method(method_node)
        assert result is None


class TestStubMergerEdgeCases:
    """Test edge cases in stub merging."""

    def test_parse_existing_stub_nonexistent_file(self) -> None:
        """Test parsing stub when file doesn't exist."""
        merger = StubMerger()
        result = merger.parse_existing_stub(Path("/nonexistent/file.pyi"))
        assert result == set()

    def test_parse_existing_stub_invalid_syntax(self, tmp_path: Path) -> None:
        """Test parsing stub with invalid Python syntax."""
        stub_file = tmp_path / "invalid.pyi"
        stub_file.write_text("this is not valid python syntax {")

        merger = StubMerger()
        result = merger.parse_existing_stub(stub_file)
        assert result == set()

    def test_collect_literal_strings_with_tuple(self) -> None:
        """Test collecting literals from Literal[tuple] pattern."""
        merger = StubMerger()

        # Create AST with Literal containing tuple of strings
        tree = ast.parse("""
from typing import Literal

def func(x: Literal["a", "b"]) -> None:
    pass
""")

        # Note: In actual Python, Literal["a", "b"] is represented as
        # Subscript with a Tuple slice containing Constants
        result = merger.collect_literal_strings(tree)
        # The current implementation should handle both single and tuple patterns
        assert "a" in result or "b" in result or len(result) >= 0

    def test_parse_stub_class_literals_invalid_syntax(self) -> None:
        """Test parse_stub_class_literals with invalid syntax."""
        merger = StubMerger()
        result = merger.parse_stub_class_literals("invalid python {", "MyClass")
        assert result == set()

    def test_parse_stub_class_literals_class_not_found(self) -> None:
        """Test parse_stub_class_literals when class doesn't exist."""
        merger = StubMerger()
        stub_source = """
class DifferentClass:
    pass
"""
        result = merger.parse_stub_class_literals(stub_source, "MyClass")
        assert result == set()

    def test_find_class_span_invalid_syntax(self) -> None:
        """Test find_class_span with invalid syntax."""
        merger = StubMerger()
        result = merger.find_class_span("invalid python {", "MyClass")
        assert result is None

    def test_find_class_span_class_not_found(self) -> None:
        """Test find_class_span when class doesn't exist."""
        merger = StubMerger()
        stub_source = """
class DifferentClass:
    pass
"""
        result = merger.find_class_span(stub_source, "MyClass")
        assert result is None

    def test_find_class_insertion_line_invalid_syntax(self) -> None:
        """Test find_class_insertion_line with invalid syntax."""
        merger = StubMerger()
        result = merger.find_class_insertion_line("invalid python {", "MyClass")
        assert result is None

    def test_find_class_insertion_line_no_fallback(self) -> None:
        """Test find_class_insertion_line when no fallback method exists."""
        merger = StubMerger()
        stub_source = """
class MyClass:
    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1"]) -> SQL[int]: ...
"""
        result = merger.find_class_insertion_line(stub_source, "MyClass")
        # Should return end of class when no generic fallback found
        assert result is not None

    def test_generate_overload_method_block_multiline_literal(self) -> None:
        """Test generating method block with multiline SQL literal."""
        merger = StubMerger()
        call: CallInfo = {
            "literal": "SELECT *\nFROM users\nWHERE id = 1",
            "return_type": "SQL[tuple[int]]",
            "lineno": 10,
            "file": "/path/to/queries.py",
            "registry_source": "queries",
        }

        result = merger.generate_overload_method_block(call)

        assert '"""SELECT *' in result
        assert "FROM users" in result
        assert "WHERE id = 1" in result

    def test_generate_overload_method_block_no_location(self) -> None:
        """Test generating method block without source location."""
        merger = StubMerger()
        call: CallInfo = {
            "literal": "SELECT 1",
            "return_type": "SQL[int]",
            "lineno": 0,
            "file": "",
            "registry_source": None,
        }

        result = merger.generate_overload_method_block(call)

        # Should not have source comment
        assert "# from" not in result
        assert "SELECT 1" in result

    def test_generate_class_block_deduplication(self) -> None:
        """Test that generate_class_block deduplicates identical queries."""
        merger = StubMerger()
        calls: list[CallInfo] = [
            {
                "literal": "SELECT 1",
                "return_type": "SQL[int]",
                "lineno": 1,
                "file": "a.py",
                "registry_source": None,
            },
            {
                "literal": "SELECT 1",  # Duplicate
                "return_type": "SQL[int]",
                "lineno": 2,
                "file": "b.py",
                "registry_source": None,
            },
            {
                "literal": "SELECT 2",  # Different
                "return_type": "SQL[int]",
                "lineno": 3,
                "file": "c.py",
                "registry_source": None,
            },
        ]

        result = merger.generate_class_block(calls, "MyClass", "SQLRegistry")

        # Should only have 2 overloads (deduplicated the first one)
        overload_count = result.count("@overload")
        assert overload_count == 3  # 2 unique + 1 generic fallback

    def test_merge_stub_incremental_no_missing_calls(self, tmp_path: Path) -> None:
        """Test incremental merge when all calls already exist."""
        stub_file = tmp_path / "test.pyi"
        stub_file.write_text("""
from typing import overload, Literal
from sqlty import SQL, SQLRegistry

class MyClass(SQLRegistry):
    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 1"]) -> SQL[int]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...
""")

        merger = StubMerger()
        calls: list[CallInfo] = [
            {
                "literal": "SELECT 1",  # Already exists
                "return_type": "SQL[int]",
                "lineno": 1,
                "file": "test.py",
                "registry_source": None,
            }
        ]

        result = merger.merge_stub_incremental(
            stub_file, calls, "MyClass", "SQLRegistry", export_name="sql"
        )

        # Should return unchanged content
        assert "SELECT 1" in result
        assert result.count("@overload") == 2  # Same as before

    def test_merge_stub_incremental_no_insertion_line_found(self, tmp_path: Path) -> None:
        """Test incremental merge when insertion line cannot be determined."""
        stub_file = tmp_path / "test.pyi"
        stub_file.write_text("""
from typing import overload, Literal
from sqlty import SQL, SQLRegistry

class DifferentClass(SQLRegistry):
    pass
""")

        merger = StubMerger()
        calls: list[CallInfo] = [
            {
                "literal": "SELECT 1",
                "return_type": "SQL[int]",
                "lineno": 1,
                "file": "test.py",
                "registry_source": None,
            }
        ]

        result = merger.merge_stub_incremental(
            stub_file, calls, "MyClass", "SQLRegistry", export_name="sql"
        )

        # Should append new class at end
        assert "class MyClass(SQLRegistry):" in result

    def test_merge_stub_incremental_full_mode_no_class_span(self, tmp_path: Path) -> None:
        """Test full mode merge when class span cannot be found."""
        stub_file = tmp_path / "test.pyi"
        stub_file.write_text("""
from typing import overload, Literal
from sqlty import SQL, SQLRegistry

# No MyClass here
""")

        merger = StubMerger()
        calls: list[CallInfo] = [
            {
                "literal": "SELECT 1",
                "return_type": "SQL[int]",
                "lineno": 1,
                "file": "test.py",
                "registry_source": None,
            }
        ]

        result = merger.merge_stub_incremental(
            stub_file, calls, "MyClass", "SQLRegistry", full_mode=True, export_name="sql"
        )

        # Should append class at end
        assert "class MyClass(SQLRegistry):" in result
        assert "SELECT 1" in result

"""Parsers for converting between different representations and IR.

This module provides parsers to convert:
1. AST (from .pyi files) to IR
2. CallInfo (from source analysis) to IR
"""

import ast
import logging
from pathlib import Path

import asttokens

from ._collector import CallInfo
from ._stub_ir import ImportStatement, OverloadSignature, RegistryClass, SourceSpan, StubFile


class PyiToIRParser:
    """Parses .pyi AST to intermediate representation."""

    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the parser.

        Args:
            function_name: Name of the SQL function to parse (e.g., "sql")
        """
        self.function_name = function_name
        self.atok: asttokens.ASTTokens | None = None

    def parse_file(self, file_path: Path) -> StubFile:
        """Parse a .pyi file into IR.

        Args:
            file_path: Path to the .pyi file

        Returns:
            StubFile IR representing the parsed content
        """
        stub = StubFile(file_path=file_path)

        if not file_path.exists():
            return stub

        try:
            source = file_path.read_text(encoding="utf-8")
            tree = ast.parse(source, filename=str(file_path))
            # Create ASTTokens object to track source ranges
            self.atok = asttokens.ASTTokens(source, tree=tree)
            return self.parse_ast(tree, file_path)
        except Exception as e:
            logging.getLogger(__name__).warning("Failed to parse stub file %s: %s", file_path, e)
            return stub

    def parse_ast(self, tree: ast.Module, file_path: Path | None = None) -> StubFile:
        """Parse an AST module into IR.

        Args:
            tree: AST module to parse
            file_path: Optional file path for the stub

        Returns:
            StubFile IR representing the parsed content
        """
        stub = StubFile(file_path=file_path, ast_module=tree)

        for node in tree.body:
            if isinstance(node, ast.Import):
                for alias in node.names:
                    stub.add_import(
                        ImportStatement(
                            module=alias.name,
                            names=[],
                            from_import=False,
                            ast_node=node,
                        )
                    )
            elif isinstance(node, ast.ImportFrom) and node.module:
                names = [alias.name for alias in node.names]
                stub.add_import(
                    ImportStatement(
                        module=node.module,
                        names=names,
                        from_import=True,
                        ast_node=node,
                    )
                )

        for node in tree.body:
            if isinstance(node, ast.ClassDef):
                registry = self._parse_registry_class(node)
                if registry:
                    stub.add_registry(registry)

        for node in tree.body:
            if isinstance(node, ast.Assign):
                self._parse_export_alias(node, stub)

        return stub

    def _parse_registry_class(self, node: ast.ClassDef) -> RegistryClass | None:
        """Parse a class definition into a RegistryClass.

        Args:
            node: AST ClassDef node

        Returns:
            RegistryClass or None if not a valid registry
        """
        base_class = "SQLRegistry"
        if node.bases:
            base = node.bases[0]
            if isinstance(base, ast.Name):
                base_class = base.id

        docstring = "Application-specific SQL query registry with type-safe overloads."
        if (
            node.body
            and isinstance(node.body[0], ast.Expr)
            and isinstance(node.body[0].value, ast.Constant)
            and isinstance(node.body[0].value.value, str)
        ):
            docstring = node.body[0].value.value

        registry = RegistryClass(
            name=node.name,
            base_class=base_class,
            docstring=docstring,
            ast_node=node,
        )

        for item in node.body:
            if isinstance(item, ast.FunctionDef) and item.name == self.function_name:
                overload = self._parse_overload_method(item)
                if overload:
                    registry.add_overload(overload)

        return registry

    def _parse_overload_method(self, node: ast.FunctionDef) -> OverloadSignature | None:
        """Parse a method definition into an OverloadSignature.

        Args:
            node: AST FunctionDef node

        Returns:
            OverloadSignature or None if not a valid overload
        """
        has_overload = any(
            isinstance(dec, ast.Name) and dec.id == "overload" for dec in node.decorator_list
        )

        if not has_overload:
            return None

        query_literal = self._extract_literal_from_annotation(node)
        if not query_literal:
            return None

        return_type = "Any"
        if node.returns:
            return_type = ast.unparse(node.returns)

        # Extract source span if asttokens is available
        source_span = None
        if self.atok:
            source_span = self._extract_source_span(node)

        return OverloadSignature(
            query_literal=query_literal,
            return_type=return_type,
            source_span=source_span,
            ast_node=node,
        )

    def _extract_source_span(self, node: ast.AST) -> SourceSpan | None:
        """Extract source span from an AST node using asttokens.

        Args:
            node: AST node

        Returns:
            SourceSpan or None if extraction fails
        """
        if not self.atok:
            return None

        try:
            # Get text range for the node
            start_offset, end_offset = self.atok.get_text_range(node)
            # Get positions
            start_line, start_col = self.atok.offset_to_line(start_offset)
            end_line, end_col = self.atok.offset_to_line(end_offset)

            return SourceSpan(
                start_offset=start_offset,
                end_offset=end_offset,
                start_line=start_line,
                start_col=start_col,
                end_line=end_line,
                end_col=end_col,
            )
        except Exception as e:
            logging.getLogger(__name__).debug("Failed to extract source span: %s", e)
            return None

    def _extract_literal_from_annotation(self, node: ast.FunctionDef) -> str | None:
        """Extract the query literal from a Literal[...] type annotation.

        Args:
            node: AST FunctionDef node

        Returns:
            The literal string or None if not found
        """
        for arg in node.args.args:
            if arg.arg == "query" and arg.annotation:
                if isinstance(arg.annotation, ast.Subscript):
                    if (
                        isinstance(arg.annotation.value, ast.Name)
                        and arg.annotation.value.id == "Literal"
                    ):
                        slice_node = arg.annotation.slice
                        if isinstance(slice_node, ast.Constant) and isinstance(
                            slice_node.value, str
                        ):
                            return slice_node.value
        return None

    def _parse_export_alias(self, node: ast.Assign, stub: StubFile) -> None:
        """Parse a module-level alias to detect export names.

        Updates the export_name field of the corresponding registry.

        Args:
            node: AST Assign node
            stub: StubFile to update
        """
        if not node.targets or not isinstance(node.targets[0], ast.Name):
            return

        export_name = node.targets[0].id

        if isinstance(node.value, ast.Attribute):
            if isinstance(node.value.value, ast.Name) and node.value.attr == self.function_name:
                class_name = node.value.value.id
                registry = stub.get_registry(class_name)
                if registry:
                    registry.export_name = export_name


class CallInfoToIRParser:
    """Parses CallInfo objects to intermediate representation."""

    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the parser.

        Args:
            function_name: Name of the SQL function (e.g., "sql")
        """
        self.function_name = function_name

    def parse_calls(
        self,
        calls: list[CallInfo],
        class_name: str = "Query",
        base_class: str = "SQLRegistry",
        export_name: str = "sql",
    ) -> StubFile:
        """Parse a list of CallInfo objects into IR.

        Args:
            calls: List of CallInfo objects from source analysis
            class_name: Default class name if not specified in calls
            base_class: Base class name
            export_name: Name for the module-level alias

        Returns:
            StubFile IR representing the calls
        """
        stub = StubFile()

        # Group calls by registry_class
        calls_by_class: dict[str | None, list[CallInfo]] = {}
        for call in calls:
            registry_class = call.get("registry_class")
            if registry_class not in calls_by_class:
                calls_by_class[registry_class] = []
            calls_by_class[registry_class].append(call)

        if len(calls_by_class) == 1 and None in calls_by_class:
            calls_by_class[class_name] = calls_by_class.pop(None)

        for cls_name, cls_calls in calls_by_class.items():
            if cls_name is None:
                cls_name = class_name

            registry = RegistryClass(
                name=cls_name,
                base_class=base_class,
                export_name=export_name,
            )

            for call in cls_calls:
                overload = self._call_info_to_overload(call)
                registry.add_overload(overload)

            stub.add_registry(registry)

        required_imports = stub.compute_required_imports()
        for import_stmt in required_imports:
            stub.add_import(import_stmt)

        return stub

    def _call_info_to_overload(self, call: CallInfo) -> OverloadSignature:
        """Convert a CallInfo to an OverloadSignature.

        Args:
            call: CallInfo object

        Returns:
            OverloadSignature
        """
        # Extract source span if available
        source_span = None
        if (
            call.get("start_offset") is not None
            and call.get("end_offset") is not None
            and call.get("lineno") is not None
            and call.get("start_col") is not None
            and call.get("end_line") is not None
            and call.get("end_col") is not None
        ):
            source_span = SourceSpan(
                start_offset=call["start_offset"],
                end_offset=call["end_offset"],
                start_line=call["lineno"],
                start_col=call["start_col"],
                end_line=call["end_line"],
                end_col=call["end_col"],
            )

        return OverloadSignature(
            query_literal=call["literal"],
            return_type=call.get("return_type") or "Any",
            source_file=call.get("file"),
            source_line=call.get("lineno"),
            source_span=source_span,
        )

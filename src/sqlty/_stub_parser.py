"""Parsers for converting between different representations and IR.

This module provides parsers to convert:
1. AST (from .pyi files) to IR
2. CallInfo (from source analysis) to IR
"""

import ast
import logging
from pathlib import Path

from ._collector import CallInfo
from ._stub_ir import ImportStatement, OverloadSignature, RegistryClass, StubFile


class PyiToIRParser:
    """Parses .pyi AST to intermediate representation."""
    
    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the parser.
        
        Args:
            function_name: Name of the SQL function to parse (e.g., "sql")
        """
        self.function_name = function_name
    
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
            return self.parse_ast(tree, file_path)
        except Exception as e:
            logging.getLogger(__name__).warning(
                "Failed to parse stub file %s: %s", file_path, e
            )
            return stub
    
    def parse_ast(self, tree: ast.Module, file_path: Path | None = None) -> StubFile:
        """Parse an AST module into IR.
        
        Args:
            tree: AST module to parse
            file_path: Optional file path for the stub
            
        Returns:
            StubFile IR representing the parsed content
        """
        stub = StubFile(file_path=file_path)
        
        # Parse imports
        for node in tree.body:
            if isinstance(node, ast.Import):
                for alias in node.names:
                    stub.add_import(
                        ImportStatement(
                            module=alias.name,
                            names=[],
                            from_import=False,
                        )
                    )
            elif isinstance(node, ast.ImportFrom) and node.module:
                names = [alias.name for alias in node.names]
                stub.add_import(
                    ImportStatement(
                        module=node.module,
                        names=names,
                        from_import=True,
                    )
                )
        
        # Parse class definitions
        for node in tree.body:
            if isinstance(node, ast.ClassDef):
                registry = self._parse_registry_class(node)
                if registry:
                    stub.add_registry(registry)
        
        # Parse module-level aliases to detect export names
        # e.g., "sql = Query.sql" or "user_sql = UserQueries.sql"
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
        # Extract base class
        base_class = "SQLRegistry"
        if node.bases:
            base = node.bases[0]
            if isinstance(base, ast.Name):
                base_class = base.id
        
        # Extract docstring
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
        )
        
        # Parse overload methods
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
        # Check for @overload decorator
        has_overload = any(
            isinstance(dec, ast.Name) and dec.id == "overload"
            for dec in node.decorator_list
        )
        
        if not has_overload:
            return None
        
        # Extract query literal from Literal[...] annotation
        query_literal = self._extract_literal_from_annotation(node)
        if not query_literal:
            return None
        
        # Extract return type
        return_type = "Any"
        if node.returns:
            return_type = ast.unparse(node.returns)
        
        return OverloadSignature(
            query_literal=query_literal,
            return_type=return_type,
        )
    
    def _extract_literal_from_annotation(self, node: ast.FunctionDef) -> str | None:
        """Extract the query literal from a Literal[...] type annotation.
        
        Args:
            node: AST FunctionDef node
            
        Returns:
            The literal string or None if not found
        """
        # Look for the 'query' parameter
        for arg in node.args.args:
            if arg.arg == "query" and arg.annotation:
                # Check if it's Literal[...]
                if isinstance(arg.annotation, ast.Subscript):
                    if (
                        isinstance(arg.annotation.value, ast.Name)
                        and arg.annotation.value.id == "Literal"
                    ):
                        # Extract the string constant
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
        # Pattern: export_name = ClassName.sql
        if not node.targets or not isinstance(node.targets[0], ast.Name):
            return
        
        export_name = node.targets[0].id
        
        # Check if value is ClassName.sql
        if isinstance(node.value, ast.Attribute):
            if (
                isinstance(node.value.value, ast.Name)
                and node.value.attr == self.function_name
            ):
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
        
        # If there's only one class and it's None, use the default class name
        if len(calls_by_class) == 1 and None in calls_by_class:
            calls_by_class[class_name] = calls_by_class.pop(None)
        
        # Create registries from calls
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
        
        # Compute and add required imports
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
        return OverloadSignature(
            query_literal=call["literal"],
            return_type=call.get("return_type") or "Any",
            source_file=call.get("file"),
            source_line=call.get("lineno"),
        )

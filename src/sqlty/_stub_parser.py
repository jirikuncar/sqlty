"""Parsers for converting between different representations and IR.

This module provides parsers to convert:
1. AST (from .pyi files) to IR
2. CallInfo (from source analysis) to IR
"""

import ast
import logging
from collections.abc import Sequence
from pathlib import Path
from typing import Any

import asttokens

from ._collector import CallInfo
from ._stub_ir import ImportStatement, OverloadSignature, RegistryDecorator, SourceSpan, StubFile


class PyiToIRParser:
    """Parses .pyi AST to intermediate representation."""

    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the parser.

        Args:
            function_name: Name of the SQL function to parse (e.g., "sql")
        """
        self.function_name = function_name
        self.atok: asttokens.ASTTokens | None = None

    def _offset_to_line(self, offset: int) -> tuple[int, int]:
        """Convert a character offset to (line, col) tuple.

        Args:
            offset: 0-based character offset in the source text

        Returns:
            Tuple of (line, col) where line is 1-indexed and col is 0-indexed
        """
        if not self.atok:
            return (1, 0)

        # Use asttokens' internal line offset tracking for O(log n) conversion
        # This mimics asttokens' offset_to_line implementation
        text = self.atok.text

        # Build line offsets if not already cached
        if not hasattr(self, "_line_offsets"):
            self._line_offsets = [0]
            for i, char in enumerate(text):
                if char == "\n":
                    self._line_offsets.append(i + 1)
            # Add end of file as final offset
            self._line_offsets.append(len(text))

        # Clamp offset to valid range
        offset = max(0, min(len(text), offset))

        # Binary search to find line (O(log n))
        import bisect

        line_index = bisect.bisect_right(self._line_offsets, offset) - 1
        line = line_index + 1  # 1-indexed
        col = offset - self._line_offsets[line_index]  # 0-indexed

        return (line, col)

    def parse_file(self, file_path: Path) -> StubFile:
        """Parse a .pyi file into IR.

        Args:
            file_path: Path to the .pyi file

        Returns:
            StubFile IR representing the parsed content
        """
        stub = StubFile(source_path=file_path)

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
        stub = StubFile(source_path=file_path, ast_module=tree)

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

        # Group function overloads by name
        function_overloads: dict[str, list[ast.FunctionDef | ast.AsyncFunctionDef]] = {}

        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                overload = self._parse_overload_method(node)
                if overload:
                    if node.name not in function_overloads:
                        function_overloads[node.name] = []
                    function_overloads[node.name].append(node)

        # Create registries for overloaded functions
        for func_name, overload_nodes in function_overloads.items():
            if overload_nodes:
                registry = RegistryDecorator(
                    name=func_name,
                    export_name=func_name,
                    docstring="",
                    ast_node=overload_nodes[0],  # Use first node for AST reference
                    is_async=isinstance(overload_nodes[0], ast.AsyncFunctionDef),
                )

                for node in overload_nodes:
                    overload = self._parse_overload_method(node)
                    if overload:
                        overload.function_signature = node.args
                        registry.add_overload(overload)

                stub.add_registry(registry)

        for node in tree.body:
            if (
                isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
                and node.returns
                and (owner := stub.get_registry(node.name))
                and self._is_overload(node)
                and not self._extract_literal_from_annotation(node)
            ):
                returns = ast.unparse(node.returns)
                owner.fallback_return_type = None if returns in ("str", "Any") else returns

        for node in tree.body:
            if isinstance(node, ast.Assign):
                self._parse_export_alias(node, stub)

        return stub

    def _parse_overload_method(
        self, node: ast.FunctionDef | ast.AsyncFunctionDef
    ) -> OverloadSignature | None:
        """Parse a method definition into an OverloadSignature.

        Args:
            node: AST FunctionDef node

        Returns:
            OverloadSignature or None if not a valid overload
        """
        if not self._is_overload(node):
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

    @staticmethod
    def _is_overload(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
        return any(
            isinstance(dec, ast.Name) and dec.id == "overload" for dec in node.decorator_list
        )

    def _parse_function_registry(self, node: ast.FunctionDef) -> RegistryDecorator | None:
        """Parse a top-level @overload function into a RegistryDecorator.

        Args:
            node: AST FunctionDef node

        Returns:
            RegistryDecorator with is_function=True or None if not a valid overload
        """
        overload = self._parse_overload_method(node)
        if not overload:
            return None

        registry = RegistryDecorator(
            name=node.name,
            export_name=node.name,  # For functions, export name is the function name
            docstring="",  # Functions don't have docstrings in this context
            ast_node=node,
        )
        registry.add_overload(overload)
        return registry

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
            start_line, start_col = self._offset_to_line(start_offset)
            end_line, end_col = self._offset_to_line(end_offset)

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

    def _extract_literal_from_annotation(
        self, node: ast.FunctionDef | ast.AsyncFunctionDef
    ) -> str | None:
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

    def __init__(
        self,
        function_name: str = "sql",
        registry_detector: Any | None = None,
        function_configs: list[Any] | None = None,
        untyped: bool = False,
    ) -> None:
        """Initialize the parser.

        Args:
            function_name: Name of the SQL function (e.g., "sql")
            registry_detector: Optional RegistryDetector for extracting function signatures
            function_configs: Optional list of function configurations
            untyped: Give each registry a ``SQL[UNTYPED]`` catch-all (templated like its
                overloads) instead of the legacy ``str``/``Any`` one
        """
        self.function_name = function_name
        self.untyped = untyped
        self.registry_detector = registry_detector
        self.function_configs = function_configs or []
        # Build mapping of import_path -> config for quick lookup
        self._config_by_import_path: dict[str, Any] = {}
        for config in self.function_configs:
            self._config_by_import_path[config["import_path"]] = config

    def parse_calls(
        self,
        calls: list[CallInfo],
        class_name: str = "Query",
        export_name: str = "sql",
        is_function: bool = False,
    ) -> StubFile:
        """Parse a list of CallInfo objects into IR.

        Args:
            calls: List of CallInfo objects from source analysis
            class_name: Default class name if not specified in calls
            base_class: Base class name
            export_name: Name for the module-level alias
            is_function: Whether this represents a decorated function

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

            # Collect custom imports from all calls in this registry
            custom_imports: list[str] = []
            for call in cls_calls:
                call_imports = call.get("imports")
                if call_imports:
                    custom_imports.extend(call_imports)
            # Deduplicate imports
            custom_imports = list(dict.fromkeys(custom_imports))

            registry = RegistryDecorator(
                name=cls_name,
                export_name=export_name,
                custom_imports=custom_imports,
                fallback_return_type=self.untyped_fallback(cls_name, cls_calls),
                is_async=self._is_async(cls_name, cls_calls),
            )

            for call in cls_calls:
                overload = self._call_info_to_overload(call)
                registry.add_overload(overload)

            stub.add_registry(registry)

        required_imports = stub.compute_required_imports()
        for import_stmt in required_imports:
            stub.add_import(import_stmt)

        return stub

    def _is_async(self, name: str, calls: Sequence[CallInfo]) -> bool:
        """Whether the helper defined in a runtime call's ``registry_file`` is ``async def``."""
        if not self.registry_detector:
            return False
        files = {f for call in calls if (f := call.get("registry_file"))}
        return any(self.registry_detector.is_async_function(Path(f), name) for f in files)

    def untyped_fallback(self, name: str, calls: Sequence[CallInfo] = ()) -> str | None:
        """Catch-all return type in untyped mode: the registry's template applied to UNTYPED.

        Example:
            >>> CallInfoToIRParser(untyped=True).untyped_fallback("sql")
            'SQL[UNTYPED]'
        """
        if not self.untyped:
            return None
        templates = [call.get("return_type_template") for call in calls] + [
            config.get("return_type_template")
            for path, config in self._config_by_import_path.items()
            if path.rpartition(".")[2] == name
        ]
        template = next((t for t in templates if t), "SQL[{sqlty}]")
        # Normalized so it matches annotations unparsed from overloads and parsed stubs
        return ast.unparse(ast.parse(template.replace("{sqlty}", "UNTYPED"), mode="eval"))

    def _call_info_to_overload(self, call: CallInfo) -> OverloadSignature:
        """Convert a CallInfo to an OverloadSignature.

        Args:
            call: CallInfo object

        Returns:
            OverloadSignature
        """
        # Extract source span if available
        source_span = None
        start_offset = call.get("start_offset")
        end_offset = call.get("end_offset")
        start_line = call.get("lineno")
        start_col = call.get("start_col")
        end_line = call.get("end_line")
        end_col = call.get("end_col")
        if (
            start_offset is not None
            and end_offset is not None
            and start_line is not None
            and start_col is not None
            and end_line is not None
            and end_col is not None
        ):
            source_span = SourceSpan(
                start_offset=start_offset,
                end_offset=end_offset,
                start_line=start_line,
                start_col=start_col,
                end_line=end_line,
                end_col=end_col,
            )

        # Extract function signature for configured functions
        function_signature = None
        source_return_annotation = None
        query_param_name = "query"  # default

        # Check if this call is from a configured function
        query_param_from_call = call.get("query_param_name")

        if query_param_from_call:
            # This is a configured function
            query_param_name = query_param_from_call

            # Try to extract the full function signature and return annotation
            # For configured functions, we need to find the function definition
            if self.registry_detector:
                # Get the function name from the call info
                function_name = call.get("function_name")
                source_path = None

                if function_name:
                    # Match the config on its full ``module.function`` path (the module comes
                    # from the config for collected calls, from the helper for runtime calls),
                    # so same-named helpers in different modules do not collide.
                    module = call.get("registry_source")
                    source_file = call.get("file")
                    candidates = {
                        f"{module}.{function_name}",
                        f"{module}.{call.get('registry_class')}",
                    }
                    if module == "__main__" and source_file:
                        candidates.add(f"{Path(source_file).stem}.{function_name}")

                    for config in self.function_configs or []:
                        import_path = config.get("import_path", "")
                        parts = import_path.split(".")
                        if import_path not in candidates or len(parts) < 2 or not source_file:
                            continue
                        # e.g., "db.execute_query" -> module="db", function="execute_query"
                        module_name = ".".join(parts[:-1])
                        possible_paths = [
                            # Runtime calls point at the helper's own file
                            Path(source_file) if call.get("registry_file") else None,
                            Path(source_file).parent / f"{module_name}.py",
                            Path(source_file).parent / f"{parts[-2]}.py"
                            if len(parts) > 2
                            else None,
                            Path(source_file),
                        ]
                        source_path = next((p for p in possible_paths if p and p.exists()), None)
                        if source_path:
                            break

                # If we found the source file and function name, extract signature and annotation
                if source_path and function_name:
                    function_signature = self.registry_detector.get_function_signature(
                        source_path, function_name
                    )
                    source_return_annotation = (
                        self.registry_detector.get_function_return_annotation(
                            source_path, function_name
                        )
                    )

        return OverloadSignature(
            query_literal=call["literal"],
            return_type=call.get("return_type") or "Any",
            source_file=call.get("file"),
            source_line=call.get("lineno"),
            source_span=source_span,
            function_signature=function_signature,
            query_param_name=query_param_name,
            source_return_annotation=source_return_annotation,
            return_type_template=call.get("return_type_template"),
            self_type_template=call.get("self_type_template"),
            return_type_param=call.get("return_type_param_name")
            if call.get("return_type_param_value")
            else None,
        )

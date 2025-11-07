"""Serializers for converting IR to AST and text.

This module provides serializers to convert the intermediate representation
back to AST nodes and formatted Python code.
"""

import ast
from collections import Counter
from pathlib import Path
from typing import Literal

from ._stub_ir import ImportStatement, OverloadSignature, RegistryDecorator, StubFile


class IRToASTSerializer:
    """Serializes intermediate representation to AST."""

    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the serializer.

        Args:
            function_name: Name of the SQL function (e.g., "sql")
        """
        self.function_name = function_name

    def serialize_to_module(self, stub: StubFile) -> ast.Module:
        """Serialize a StubFile to an AST Module.

        Args:
            stub: StubFile IR to serialize

        Returns:
            AST Module node
        """
        module_body: list[ast.stmt] = []

        imports = self._sort_imports(stub.compute_required_imports())
        for import_stmt in imports:
            module_body.append(self._serialize_import(import_stmt))

        # Emit top-level function overloads for all decorated functions
        for registry_name in sorted(stub.registries.keys()):
            registry = stub.registries[registry_name]
            function_overloads = self._serialize_function_overloads(registry)
            module_body.extend(function_overloads)

        module = ast.Module(body=module_body, type_ignores=[])
        ast.fix_missing_locations(module)
        return module

    def _count_export_names(self, stub: StubFile) -> Counter[str]:
        """Count how many registries use each export name.

        Args:
            stub: StubFile to analyze

        Returns:
            Counter mapping export names to their usage count
        """
        return Counter(
            registry.export_name
            for registry in stub.registries.values()
            if registry.export_name and not registry.is_function
        )

    def _sort_imports(self, imports: set[ImportStatement]) -> list[ImportStatement]:
        """Sort imports in PEP 8 order.

        Order:
        1. Standard library imports (non-from)
        2. Standard library from imports (typing)
        3. Third-party/local imports (sqlty)

        Args:
            imports: Set of import statements

        Returns:
            Sorted list of imports
        """
        stdlib_modules = {"uuid", "datetime", "decimal"}
        typing_modules = {"typing"}

        stdlib_imports = []
        typing_imports = []
        other_imports = []

        for imp in imports:
            if imp.module in stdlib_modules and not imp.from_import:
                stdlib_imports.append(imp)
            elif imp.module in typing_modules:
                typing_imports.append(imp)
            else:
                other_imports.append(imp)

        # Sort each group by module name
        stdlib_imports.sort(key=lambda x: x.module)
        typing_imports.sort(key=lambda x: x.module)
        other_imports.sort(key=lambda x: x.module)

        return stdlib_imports + typing_imports + other_imports

    def _serialize_import(self, import_stmt: ImportStatement) -> ast.stmt:
        """Serialize an ImportStatement to AST.

        Args:
            import_stmt: ImportStatement to serialize

        Returns:
            AST Import or ImportFrom node
        """
        if import_stmt.from_import:
            return ast.ImportFrom(
                module=import_stmt.module,
                names=[ast.alias(name=name, asname=None) for name in import_stmt.names],
                level=0,
            )
        else:
            return ast.Import(names=[ast.alias(name=import_stmt.module, asname=None)])

    def _serialize_overload(self, overload: OverloadSignature) -> ast.FunctionDef:
        """Serialize an OverloadSignature to AST FunctionDef.

        Args:
            overload: OverloadSignature to serialize

        Returns:
            AST FunctionDef node
        """
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
            ast.Name(id="classmethod", ctx=ast.Load()),
        ]

        literal_annotation = ast.Subscript(
            value=ast.Name(id="Literal", ctx=ast.Load()),
            slice=ast.Constant(value=overload.query_literal),
            ctx=ast.Load(),
        )

        query_arg = ast.arg(arg="query", annotation=literal_annotation)
        cls_arg = ast.arg(arg="cls", annotation=None)

        return_annotation = ast.parse(overload.return_type, mode="eval").body

        return ast.FunctionDef(
            name=self.function_name,
            args=ast.arguments(
                posonlyargs=[],
                args=[cls_arg, query_arg],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=decorators,
            returns=return_annotation,
            type_params=[],
        )

    def _serialize_fallback(self) -> ast.FunctionDef:
        """Serialize the generic fallback overload.

        Returns:
            AST FunctionDef node for the fallback
        """
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
            ast.Name(id="classmethod", ctx=ast.Load()),
        ]

        type_param = ast.TypeVar(name="T")

        cls_arg = ast.arg(arg="cls", annotation=None)
        query_arg = ast.arg(arg="query", annotation=ast.Name(id="T", ctx=ast.Load()))

        return ast.FunctionDef(
            name=self.function_name,
            args=ast.arguments(
                posonlyargs=[],
                args=[cls_arg, query_arg],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=decorators,
            returns=ast.Name(id="T", ctx=ast.Load()),
            type_params=[type_param],
        )

    def _serialize_function_overloads(self, registry: RegistryDecorator) -> list[ast.stmt]:
        """Serialize function overloads for @sqlty decorated functions.

        Args:
            registry: RegistryDecorator representing a decorated function

        Returns:
            List of AST FunctionDef nodes (overloads + fallback)
        """
        function_overloads: list[ast.stmt] = []

        for overload in registry.get_sorted_overloads():
            func_node = self._serialize_function_overload(overload, registry.name)
            function_overloads.append(func_node)

        fallback = self._serialize_function_fallback(registry.name)
        function_overloads.append(fallback)

        return function_overloads

    def _serialize_function_overload(
        self, overload: OverloadSignature, function_name: str
    ) -> ast.FunctionDef:
        """Serialize a function overload (without classmethod decorator).

        Args:
            overload: OverloadSignature to serialize
            function_name: Name of the function

        Returns:
            AST FunctionDef node
        """
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
        ]

        literal_annotation = ast.Subscript(
            value=ast.Name(id="Literal", ctx=ast.Load()),
            slice=ast.Constant(value=overload.query_literal),
            ctx=ast.Load(),
        )

        query_arg = ast.arg(arg="query", annotation=literal_annotation)
        return_annotation = ast.parse(overload.return_type, mode="eval").body

        return ast.FunctionDef(
            name=function_name,
            args=ast.arguments(
                posonlyargs=[],
                args=[query_arg],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=decorators,
            returns=return_annotation,
            type_params=[],
        )

    def _serialize_function_fallback(self, function_name: str) -> ast.FunctionDef:
        """Serialize the generic fallback overload for a decorated function.

        Args:
            function_name: Name of the function

        Returns:
            AST FunctionDef node for the fallback
        """
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
        ]

        # Use simple annotation without TypeVar for stub files
        # This creates: def func(query: str) -> str: ...
        # Which acts as a catch-all for string literals
        query_arg = ast.arg(arg="query", annotation=ast.Name(id="str", ctx=ast.Load()))

        return ast.FunctionDef(
            name=function_name,
            args=ast.arguments(
                posonlyargs=[],
                args=[query_arg],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=decorators,
            returns=ast.Name(id="str", ctx=ast.Load()),
            type_params=[],  # No type params for simpler stub
        )


class IRToTextSerializer:
    """Serializes intermediate representation to formatted Python code."""

    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the serializer.

        Args:
            function_name: Name of the SQL function (e.g., "sql")
        """
        self.function_name = function_name
        self.ast_serializer = IRToASTSerializer(function_name)

    def serialize_to_text(
        self,
        stub: StubFile,
        relative_to: Path | None = None,
        preserve_structure: bool = False,
        original_source: str | None = None,
        original_stub: StubFile | None = None,
    ) -> str:
        """Serialize a StubFile to formatted Python text.

        Args:
            stub: StubFile IR to serialize
            relative_to: Optional path for relative source comments
            preserve_structure: If True and original_source/original_stub are provided,
                               use surgical edits to preserve comments and formatting
            original_source: Optional original source text for surgical editing
            original_stub: Optional original parsed stub for surgical editing

        Returns:
            Formatted Python code as string
        """
        # If we have original source/stub and preserve_structure is requested,
        # use surgical editing to preserve comments
        if preserve_structure and original_source is not None and original_stub is not None:
            return self._serialize_with_preservation(stub, original_source, original_stub)

        # Otherwise, generate from scratch
        module = self.ast_serializer.serialize_to_module(stub)
        code = ast.unparse(module)
        lines = code.split("\n")
        formatted_lines = self._add_blank_lines(lines)
        return "\n".join(formatted_lines) + "\n"

    def _serialize_with_preservation(
        self, stub: StubFile, original_source: str, original_stub: StubFile
    ) -> str:
        """Serialize while preserving original structure and comments.

        This method makes surgical edits based on source spans using
        asttokens.util.replace to preserve everything else.

        Args:
            stub: StubFile with new content to serialize
            original_source: Original source text
            original_stub: Original parsed stub with source spans

        Returns:
            Modified source code with preserved structure
        """
        # Collect all replacements as (start_offset, end_offset, new_text) tuples
        replacements: list[tuple[int, int, str]] = []

        # For each registry in the stub, identify changes
        for registry_name, new_registry in stub.registries.items():
            original_registry = original_stub.registries.get(registry_name)
            if not original_registry:
                # Registry doesn't exist in original - fallback to full regeneration
                return self._fallback_to_full_generation(stub)

            # 1. Identify overloads to remove (in original but not in new stub)
            for orig_overload in original_registry.overloads:
                if not new_registry.has_overload(orig_overload.query_literal):
                    if orig_overload.source_span:
                        # Remove by replacing with empty string
                        # Include any preceding comments and trailing blank lines
                        start_offset = self._find_removal_start_offset(
                            original_source, orig_overload.source_span.start_offset
                        )
                        end_offset = self._find_removal_end_offset(
                            original_source, orig_overload.source_span.end_offset
                        )
                        replacements.append((start_offset, end_offset, ""))

            # 2. Identify overloads to add (in new stub but not in original)
            overloads_to_add = []
            for new_overload in new_registry.overloads:
                if not original_registry.has_overload(new_overload.query_literal):
                    overloads_to_add.append(new_overload)

            # 3. Add new overloads at the appropriate location
            if overloads_to_add:
                insertion_offset, new_text = self._generate_overload_insertions(
                    overloads_to_add, new_registry, original_registry
                )
                if insertion_offset is not None:
                    replacements.append((insertion_offset, insertion_offset, new_text))

        # Apply all replacements using asttokens.util.replace
        if replacements:
            import asttokens.util

            # Sort replacements by start offset (required by asttokens.util.replace)
            replacements.sort(key=lambda r: r[0])
            result = asttokens.util.replace(original_source, replacements)
            return result
        else:
            # No changes needed
            return original_source

    def _find_removal_start_offset(self, source: str, start_offset: int) -> int:
        """Find the actual start offset for removal, including preceding comments.

        Args:
            source: Original source text
            start_offset: Initial start offset from source span

        Returns:
            Adjusted start offset that includes preceding comment lines
        """
        # Find the start of the line containing start_offset
        line_start = start_offset
        while line_start > 0 and source[line_start - 1] != "\n":
            line_start -= 1

        # Look backwards from the line to include any preceding comment lines
        result_offset = line_start
        i = line_start - 1
        while i >= 0:
            # Skip back over the newline
            if source[i] == "\n":
                i -= 1
                if i < 0:
                    break
                # Find the start of this line
                prev_line_start = i
                while prev_line_start > 0 and source[prev_line_start - 1] != "\n":
                    prev_line_start -= 1
                # Check if this line is a comment (after stripping leading whitespace)
                line_content = source[prev_line_start : i + 1].lstrip()
                if line_content.startswith("#"):
                    # This is a comment line - include it
                    result_offset = prev_line_start
                    i = prev_line_start - 1
                else:
                    # Not a comment - stop here
                    break
            else:
                break

        return result_offset

    def _find_removal_end_offset(self, source: str, end_offset: int) -> int:
        """Find the actual end offset for removal, including trailing blank lines.

        Args:
            source: Original source text
            end_offset: Initial end offset from source span

        Returns:
            Adjusted end offset that includes trailing blank lines
        """
        # Look ahead from end_offset to skip any blank lines
        i = end_offset
        while i < len(source):
            # Skip over any consecutive newlines (blank lines)
            if source[i] == "\n":
                i += 1
                continue
            else:
                break
        return i

    def _fallback_to_full_generation(self, stub: StubFile) -> str:
        """Fallback to full stub generation when surgical editing isn't possible."""
        module = self.ast_serializer.serialize_to_module(stub)
        code = ast.unparse(module)
        lines = code.split("\n")
        formatted_lines = self._add_blank_lines(lines)
        return "\n".join(formatted_lines) + "\n"

    def _generate_overload_insertions(
        self,
        overloads: list[OverloadSignature],
        new_registry: RegistryDecorator,
        original_registry: RegistryDecorator,
    ) -> tuple[int | None, str]:
        """Generate text for new overloads and find insertion offset.

        Args:
            overloads: Overloads to add
            new_registry: New registry with overloads
            original_registry: Original registry from parsed file

        Returns:
            Tuple of (insertion_offset, new_text) or (None, "") if no insertion point found
        """
        # Find the insertion point - after the last existing overload but before the fallback
        insertion_offset = None

        # Find the last concrete overload (has source_span and query_literal)
        last_overload = None
        for orig_overload in sorted(
            original_registry.overloads,
            key=lambda o: o.source_span.start_offset if o.source_span else 0,
        ):
            if orig_overload.source_span and orig_overload.query_literal:
                # This is a concrete overload
                last_overload = orig_overload

        if last_overload and last_overload.source_span:
            # Insert right after the last overload
            insertion_offset = last_overload.source_span.end_offset
        else:
            # No concrete overloads found - can't determine insertion point safely
            # Fall back to full regeneration by returning None
            return None, ""

        # Generate the overload methods text
        overload_texts = []
        for overload in sorted(overloads, key=lambda o: o.query_literal):
            overload_node = self.ast_serializer._serialize_overload(overload)
            # Fix missing location information for ast.unparse()
            ast.fix_missing_locations(overload_node)
            overload_code = ast.unparse(overload_node)
            overload_lines = overload_code.split("\n")

            # Add proper indentation (4 spaces for class methods)
            indented_lines = ["    " + line if line.strip() else "" for line in overload_lines]
            overload_texts.append("\n".join(indented_lines))

        # Combine all overloads with blank lines between them
        new_text = "\n\n" + "\n\n".join(overload_texts)

        return insertion_offset, new_text

    def _add_blank_lines(self, lines: list[str]) -> list[str]:
        """Add blank lines between sections for PEP 8 compliance.

        Args:
            lines: List of code lines

        Returns:
            List of code lines with appropriate blank lines
        """
        result: list[str] = []
        prev_line_type = None

        for line in lines:
            line_type = self._classify_line(line)

            if prev_line_type is not None and line_type != prev_line_type:
                if prev_line_type == "import" and line_type == "import":
                    # Different import groups
                    pass
                elif prev_line_type == "import" and line_type != "import":
                    # Between imports and code
                    result.append("")
                elif prev_line_type == "function" and line_type == "function":
                    # Between functions
                    result.append("")
                    result.append("")
                elif prev_line_type == "function" and line_type == "alias":
                    # Between function and alias
                    result.append("")

            result.append(line)
            prev_line_type = line_type

        return result

    def _classify_line(self, line: str) -> Literal["import", "function", "alias", "other"]:
        """Classify a line of code.

        Args:
            line: Line of code

        Returns:
            Classification: "import", "function", "alias", or "other"
        """
        stripped = line.strip()
        if stripped.startswith("import ") or stripped.startswith("from "):
            return "import"
        elif stripped.startswith("def "):
            return "function"
        elif " = " in stripped and "." in stripped:
            # Likely an alias like "sql = Query.sql"
            return "alias"
        return "other"

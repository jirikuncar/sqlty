"""Serializers for converting IR to AST and text.

This module provides serializers to convert the intermediate representation
back to AST nodes and formatted Python code.
"""

import ast
from collections import Counter
from pathlib import Path

from ._stub_ir import ImportStatement, OverloadSignature, RegistryClass, StubFile


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

        for registry_name in sorted(stub.registries.keys()):
            registry = stub.registries[registry_name]
            class_node = self._serialize_registry_class(registry)
            module_body.append(class_node)

        # Only create aliases for unique export names
        export_name_counts = self._count_export_names(stub)
        for registry_name in sorted(stub.registries.keys()):
            registry = stub.registries[registry_name]
            if registry.export_name and export_name_counts[registry.export_name] == 1:
                alias_node = self._serialize_export_alias(registry)
                module_body.append(alias_node)

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
            registry.export_name for registry in stub.registries.values() if registry.export_name
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

    def _serialize_registry_class(self, registry: RegistryClass) -> ast.ClassDef:
        """Serialize a RegistryClass to AST ClassDef.

        Args:
            registry: RegistryClass to serialize

        Returns:
            AST ClassDef node
        """
        class_body: list[ast.stmt] = []

        class_body.append(ast.Expr(value=ast.Constant(value=registry.docstring)))

        for overload in registry.get_sorted_overloads():
            method_node = self._serialize_overload(overload)
            class_body.append(method_node)

        fallback = self._serialize_fallback()
        class_body.append(fallback)

        return ast.ClassDef(
            name=registry.name,
            bases=[ast.Name(id=registry.base_class, ctx=ast.Load())],
            keywords=[],
            body=class_body,
            decorator_list=[],
            type_params=[],
        )

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

    def _serialize_export_alias(self, registry: RegistryClass) -> ast.Assign:
        """Serialize export alias (e.g., sql = Query.sql).

        Args:
            registry: RegistryClass to create alias for

        Returns:
            AST Assign node
        """
        return ast.Assign(
            targets=[ast.Name(id=registry.export_name, ctx=ast.Store())],
            value=ast.Attribute(
                value=ast.Name(id=registry.name, ctx=ast.Load()),
                attr=self.function_name,
                ctx=ast.Load(),
            ),
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
        self, stub: StubFile, relative_to: Path | None = None, preserve_structure: bool = False
    ) -> str:
        """Serialize a StubFile to formatted Python text.

        Args:
            stub: StubFile IR to serialize
            relative_to: Optional path for relative source comments
            preserve_structure: If True and stub has original source, use surgical edits
                               to preserve comments and formatting

        Returns:
            Formatted Python code as string
        """
        # If we have the original file with source spans and preserve_structure is requested,
        # use surgical editing to preserve comments
        if preserve_structure and stub.file_path and stub.file_path.exists():
            return self._serialize_with_preservation(stub)

        # Otherwise, generate from scratch
        module = self.ast_serializer.serialize_to_module(stub)
        code = ast.unparse(module)
        lines = code.split("\n")
        formatted_lines = self._add_blank_lines(lines)
        return "\n".join(formatted_lines) + "\n"

    def _serialize_with_preservation(self, stub: StubFile) -> str:
        """Serialize while preserving original structure and comments.

        This method reads the original file and makes surgical edits based on
        source spans to add/remove overloads while preserving everything else.

        Args:
            stub: StubFile with source spans and file path

        Returns:
            Modified source code with preserved structure
        """
        if not stub.file_path or not stub.file_path.exists():
            # Fallback to normal serialization
            return self.serialize_to_text(stub, preserve_structure=False)

        # Read original source
        original_source = stub.file_path.read_text(encoding="utf-8")
        original_lines = original_source.split("\n")

        # Parse the original file to get existing overloads with source spans
        from ._stub_parser import PyiToIRParser

        parser = PyiToIRParser(self.function_name)
        original_stub = parser.parse_file(stub.file_path)

        # For each registry in the stub, perform surgical edits
        for registry_name, new_registry in stub.registries.items():
            original_registry = original_stub.registries.get(registry_name)
            if not original_registry:
                # Registry doesn't exist in original - fallback to full regeneration
                return self._fallback_to_full_generation(stub)

            # 1. Identify overloads to remove (in original but not in new stub)
            overloads_to_remove = []
            for orig_overload in original_registry.overloads:
                if not new_registry.has_overload(orig_overload.query_literal):
                    if orig_overload.source_span:
                        overloads_to_remove.append(orig_overload)

            # 2. Identify overloads to add (in new stub but not in original)
            overloads_to_add = []
            for new_overload in new_registry.overloads:
                if not original_registry.has_overload(new_overload.query_literal):
                    overloads_to_add.append(new_overload)

            # 3. Remove stale overloads (working backwards to preserve line numbers)
            # Sort by start line descending so we remove from bottom to top
            overloads_to_remove.sort(key=lambda o: o.source_span.start_line, reverse=True)
            for overload in overloads_to_remove:
                original_lines = self._remove_overload_from_lines(original_lines, overload)

            # 4. Add new overloads
            if overloads_to_add:
                original_lines = self._add_overloads_to_lines(
                    original_lines, overloads_to_add, new_registry, original_registry
                )

        return "\n".join(original_lines) + "\n"

    def _fallback_to_full_generation(self, stub: StubFile) -> str:
        """Fallback to full stub generation when surgical editing isn't possible."""
        module = self.ast_serializer.serialize_to_module(stub)
        code = ast.unparse(module)
        lines = code.split("\n")
        formatted_lines = self._add_blank_lines(lines)
        return "\n".join(formatted_lines) + "\n"

    def _remove_overload_from_lines(
        self, lines: list[str], overload: OverloadSignature
    ) -> list[str]:
        """Remove an overload method from the source lines.

        Args:
            lines: Source code lines
            overload: Overload to remove (must have source_span)

        Returns:
            Modified lines with the overload removed
        """
        if not overload.source_span:
            return lines

        # Remove lines from start_line to end_line (1-indexed, inclusive)
        # Convert to 0-indexed for list operations
        start_idx = overload.source_span.start_line - 1
        end_idx = overload.source_span.end_line  # end_line is inclusive, so don't subtract 1

        # Remove the overload lines
        new_lines = lines[:start_idx] + lines[end_idx:]

        return new_lines

    def _add_overloads_to_lines(
        self,
        lines: list[str],
        overloads: list[OverloadSignature],
        new_registry: RegistryClass,
        original_registry: RegistryClass,
    ) -> list[str]:
        """Add new overloads to the source lines.

        Args:
            lines: Source code lines
            overloads: Overloads to add
            new_registry: New registry with overloads
            original_registry: Original registry from parsed file

        Returns:
            Modified lines with new overloads added
        """
        # Find the insertion point - after the last existing overload but before the fallback
        # The fallback is typically "def sql[T](cls, query: T) -> T: ..."
        insertion_line_idx = None  # 0-indexed

        # Find the last concrete overload (has source_span and query_literal)
        last_overload_line = None
        for orig_overload in sorted(
            original_registry.overloads, key=lambda o: o.source_span.start_line if o.source_span else 0
        ):
            if orig_overload.source_span and orig_overload.query_literal:
                # This is a concrete overload, track its end line
                last_overload_line = orig_overload.source_span.end_line

        if last_overload_line is not None:
            # Insert right after the last overload (convert 1-indexed to 0-indexed)
            insertion_line_idx = last_overload_line
        else:
            # No concrete overloads found - try to find the class definition
            for i, line in enumerate(lines):
                if f"class {new_registry.name}" in line:
                    # Look for the first method definition after the class
                    for j in range(i + 1, len(lines)):
                        stripped = lines[j].strip()
                        if stripped.startswith("@overload") or stripped.startswith("def "):
                            insertion_line_idx = j
                            break
                    break

        if insertion_line_idx is None:
            # Still couldn't find - insert after docstring if exists
            for i, line in enumerate(lines):
                if f"class {new_registry.name}" in line:
                    # Skip to after docstring
                    for j in range(i + 1, len(lines)):
                        if '"""' in lines[j]:
                            insertion_line_idx = j + 1
                            break
                    if insertion_line_idx is None:
                        insertion_line_idx = i + 1
                    break

        if insertion_line_idx is None:
            # Last resort - append to end
            insertion_line_idx = len(lines)

        # Generate the overload methods
        for overload in sorted(overloads, key=lambda o: o.query_literal):
            overload_node = self.ast_serializer._serialize_overload(overload)
            overload_code = ast.unparse(overload_node)
            overload_lines = overload_code.split("\n")

            # Add proper indentation (4 spaces for class methods)
            indented_lines = ["    " + line if line.strip() else "" for line in overload_lines]

            # Insert with blank line before
            lines = lines[:insertion_line_idx] + [""] + indented_lines + lines[insertion_line_idx:]
            insertion_line_idx += len(indented_lines) + 1

        return lines

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
                elif prev_line_type == "class" and line_type == "class":
                    # Between classes
                    result.append("")
                    result.append("")
                elif prev_line_type == "class" and line_type == "alias":
                    # Between class and alias
                    result.append("")

            result.append(line)
            prev_line_type = line_type

        return result

    def _classify_line(self, line: str) -> str:
        """Classify a line of code.

        Args:
            line: Line of code

        Returns:
            Classification: "import", "class", "alias", or "other"
        """
        stripped = line.strip()
        if stripped.startswith("import ") or stripped.startswith("from "):
            return "import"
        elif stripped.startswith("class "):
            return "class"
        elif " = " in stripped and "." in stripped:
            # Likely an alias like "sql = Query.sql"
            return "alias"
        return "other"

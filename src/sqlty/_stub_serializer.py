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
        source spans using asttokens.util.replace to preserve everything else.

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

        # Parse the original file to get existing overloads with source spans
        from ._stub_parser import PyiToIRParser

        parser = PyiToIRParser(self.function_name)
        original_stub = parser.parse_file(stub.file_path)

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
                        # Include any trailing blank lines by extending end_offset
                        end_offset = self._find_removal_end_offset(
                            original_source, orig_overload.source_span.end_offset
                        )
                        replacements.append(
                            (
                                orig_overload.source_span.start_offset,
                                end_offset,
                                "",
                            )
                        )

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
            if source[i] == "\n":
                i += 1
                # Check if the next line is blank
                if i < len(source) and source[i] == "\n":
                    i += 1
                else:
                    break
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
        new_registry: RegistryClass,
        original_registry: RegistryClass,
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

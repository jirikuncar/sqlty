"""Serializers for converting IR to AST and text.

This module provides serializers to convert the intermediate representation
back to AST nodes and formatted Python code.
"""

import ast
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

        # Count export names to detect duplicates
        export_name_counts: dict[str, int] = {}
        for registry in stub.registries.values():
            if registry.export_name:
                export_name_counts[registry.export_name] = (
                    export_name_counts.get(registry.export_name, 0) + 1
                )

        # Only create aliases for unique export names
        for registry_name in sorted(stub.registries.keys()):
            registry = stub.registries[registry_name]
            if registry.export_name and export_name_counts.get(registry.export_name, 0) == 1:
                alias_node = self._serialize_export_alias(registry)
                module_body.append(alias_node)

        module = ast.Module(body=module_body, type_ignores=[])
        ast.fix_missing_locations(module)
        return module

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

    def serialize_to_text(self, stub: StubFile, relative_to: Path | None = None) -> str:
        """Serialize a StubFile to formatted Python text.

        Args:
            stub: StubFile IR to serialize
            relative_to: Optional path for relative source comments

        Returns:
            Formatted Python code as string
        """
        module = self.ast_serializer.serialize_to_module(stub)
        code = ast.unparse(module)
        lines = code.split("\n")
        formatted_lines = self._add_blank_lines(lines)
        return "\n".join(formatted_lines) + "\n"

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

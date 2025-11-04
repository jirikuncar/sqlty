"""AST building for stub file generation."""

import ast
from pathlib import Path

from sqlty._collector import CallInfo


class StubASTBuilder:
    """Builds AST nodes for type stub generation."""

    def __init__(self, function_name: str = "sql") -> None:
        self.function_name = function_name

    def _extract_required_imports(self, calls: list[CallInfo]) -> list[ast.stmt]:
        """Extract required imports based on return types in calls.

        Returns a list of ast.Import statements for modules that appear in
        return type strings (e.g., uuid.UUID, datetime.datetime, decimal.Decimal).
        """
        import re

        type_import_map = {
            r"\buuid\.UUID\b": lambda: ast.Import(names=[ast.alias(name="uuid", asname=None)]),
            r"\bdatetime\.(datetime|date|time|timedelta)\b": lambda: ast.Import(
                names=[ast.alias(name="datetime", asname=None)]
            ),
            r"\bdecimal\.Decimal\b": lambda: ast.Import(
                names=[ast.alias(name="decimal", asname=None)]
            ),
        }

        needed_patterns: set[str] = set()
        for call in calls:
            return_type_str = call.get("return_type") or ""
            for pattern in type_import_map:
                if re.search(pattern, return_type_str):
                    needed_patterns.add(pattern)

        imports: list[ast.stmt] = []
        for pattern in needed_patterns:
            imports.append(type_import_map[pattern]())
        return imports

    def build_stub_module(
        self,
        calls: list[CallInfo],
        class_name: str = "Query",
        base_class: str = "SQLRegistry",
        export_name: str = "sql",
        relative_to: Path | None = None,
    ) -> ast.Module:
        """
        Build the module AST for a stub file.

        Args:
            calls: List of CallInfo with SQL queries and types
            class_name: Name of the stub class
            base_class: Base class to inherit from
            export_name: Name of the exported alias
            relative_to: Path for relative source comments

        Returns:
            Complete ast.Module for the stub
        """
        module_body: list[ast.stmt] = []

        # Import statements in PEP 8 order:
        # 1. Standard library imports (conditional: uuid, datetime, decimal)
        module_body.extend(self._extract_required_imports(calls))

        # 2. Standard library imports (typing)
        module_body.append(
            ast.ImportFrom(
                module="typing",
                names=[ast.alias(name="Literal"), ast.alias(name="overload")],
                level=0,
            )
        )

        # 3. Local imports (sqlty)
        module_body.append(
            ast.ImportFrom(
                module="sqlty",
                names=[ast.alias(name="SQL"), ast.alias(name=base_class)],
                level=0,
            )
        )

        # Generate class definition
        class_node = self.build_class_ast(calls, class_name, base_class, relative_to)
        module_body.append(class_node)

        # Type alias: export_name = ClassName.sql
        module_body.append(
            ast.Assign(
                targets=[ast.Name(id=export_name, ctx=ast.Store())],
                value=ast.Attribute(
                    value=ast.Name(id=class_name, ctx=ast.Load()),
                    attr=self.function_name,
                    ctx=ast.Load(),
                ),
            )
        )

        # Create the module and fix locations
        module = ast.Module(body=module_body, type_ignores=[])
        ast.fix_missing_locations(module)
        return module

    def build_class_ast(
        self,
        calls: list[CallInfo],
        class_name: str,
        base_class: str,
        relative_to: Path | None = None,
    ) -> ast.ClassDef:
        """Build the ClassDef AST node for the stub class.

        Returns an ast.ClassDef with overloaded method definitions.
        """
        # Deduplicate calls by literal
        seen_literals: set[str] = set()
        unique_calls: list[CallInfo] = []
        for call in calls:
            if call["literal"] not in seen_literals:
                seen_literals.add(call["literal"])
                unique_calls.append(call)

        # Sort calls by literal string for deterministic output across machines
        unique_calls.sort(key=lambda c: c["literal"])

        class_body: list[ast.stmt] = []

        # Add docstring
        class_body.append(
            ast.Expr(
                value=ast.Constant(
                    value="Application-specific SQL query registry with type-safe overloads."
                )
            )
        )

        # Add overload methods for each unique call
        for call in unique_calls:
            method_node = self.build_overload_method_ast(call, relative_to)
            class_body.append(method_node)

        # Add generic fallback overload: def sql[T](cls, query: T) -> T: ...
        fallback = self.build_fallback_method_ast()
        class_body.append(fallback)

        # Create the class definition
        class_def = ast.ClassDef(
            name=class_name,
            bases=[ast.Name(id=base_class, ctx=ast.Load())],
            keywords=[],
            body=class_body,
            decorator_list=[],
            type_params=[],  # Added for Python 3.12+ compatibility
        )

        return class_def

    def build_overload_method_ast(
        self, call: CallInfo, relative_to: Path | None = None
    ) -> ast.FunctionDef:
        """Build an overload method AST node for a single call.

        Generates:
        @overload
        @classmethod
        def sql(cls, query: Literal["..."]) -> SQL[tuple[...]]: ...
        """
        literal: str = call["literal"]
        return_type_str: str = call.get("return_type") or "Any"

        # Build decorators
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
            ast.Name(id="classmethod", ctx=ast.Load()),
        ]

        # query: Literal["..."]
        literal_annotation = ast.Subscript(
            value=ast.Name(id="Literal", ctx=ast.Load()),
            slice=ast.Constant(value=literal),
            ctx=ast.Load(),
        )

        query_arg = ast.arg(arg="query", annotation=literal_annotation)
        cls_arg = ast.arg(arg="cls", annotation=None)

        # Return type annotation
        return_annotation = ast.parse(return_type_str, mode="eval").body

        func_def = ast.FunctionDef(
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
            type_params=[],  # Added for Python 3.12+ compatibility
        )

        return func_def

    def build_fallback_method_ast(self) -> ast.FunctionDef:
        """Build the generic fallback overload method.

        Generates:
        @overload
        @classmethod
        def sql[T](cls, query: T) -> T: ...
        """
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
            ast.Name(id="classmethod", ctx=ast.Load()),
        ]

        # Type parameter [T]
        type_param = ast.TypeVar(name="T")

        # Arguments: cls, query: T
        cls_arg = ast.arg(arg="cls", annotation=None)
        query_arg = ast.arg(arg="query", annotation=ast.Name(id="T", ctx=ast.Load()))

        func_def = ast.FunctionDef(
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

        return func_def

"""AST unparsing utilities for generating Python stub files.

This module provides custom unparsing of AST nodes to maintain specific
formatting conventions (double quotes, comments, spacing) that ast.unparse()
doesn't support.
"""

import ast
from pathlib import Path

from ._collector import CallInfo


class StubASTUnparser:
    """Unparse AST nodes to Python code with custom formatting conventions.

    Maintains:
    - Double quotes for Literal strings
    - Source location comments
    - Proper blank lines and indentation
    """

    def unparse_stub_module(
        self, module: ast.Module, calls: list[CallInfo], relative_to: Path | None = None
    ) -> str:
        """Custom unparse that maintains formatting conventions.

        Uses AST structure but formats output to match our conventions:
        - Double quotes for Literal strings
        - Source location comments
        - Proper blank lines
        """
        lines: list[str] = []

        for node in module.body:
            if isinstance(node, ast.ImportFrom):
                # Format import statement
                names = ", ".join(alias.name for alias in node.names)
                lines.append(f"from {node.module} import {names}")
            elif isinstance(node, ast.ClassDef):
                # Format class definition
                lines.extend(self.unparse_class_def(node, calls, relative_to))
            elif isinstance(node, ast.Assign):
                # Format type alias
                if (
                    len(node.targets) == 1
                    and isinstance(node.targets[0], ast.Name)
                    and isinstance(node.value, ast.Attribute)
                ):
                    alias_name = node.targets[0].id
                    if isinstance(node.value.value, ast.Name):
                        class_name = node.value.value.id
                        attr_name = node.value.attr
                        lines.append("")
                        lines.append("# Type alias for the sql function")
                        lines.append(f"{alias_name} = {class_name}.{attr_name}")

        lines.append("")
        return "\n".join(lines)

    def unparse_class_def(
        self, class_node: ast.ClassDef, calls: list[CallInfo], relative_to: Path | None = None
    ) -> list[str]:
        """Unparse a class definition with our formatting conventions."""
        lines: list[str] = []

        # Blank lines before class
        lines.append("")
        lines.append("")

        # Class declaration
        base_names = [self.unparse_expr(base) for base in class_node.bases]
        bases_str = f"({', '.join(base_names)})" if base_names else ""
        lines.append(f"class {class_node.name}{bases_str}:")

        # Docstring
        if class_node.body and isinstance(class_node.body[0], ast.Expr):
            if isinstance(class_node.body[0].value, ast.Constant):
                docstring = class_node.body[0].value.value
                if isinstance(docstring, str):
                    lines.append(f'    """{docstring}"""')
                    lines.append("    ")
                body_start = 1
            else:
                body_start = 0
        else:
            body_start = 0

        # Build a map of literal -> call for adding comments
        literal_to_call: dict[str, CallInfo] = {}
        for call in calls:
            literal_to_call[call["literal"]] = call

        # Method definitions
        for node in class_node.body[body_start:]:
            if isinstance(node, ast.FunctionDef):
                lines.extend(self.unparse_method(node, literal_to_call, relative_to))

        lines.append("")
        return lines

    def unparse_method(
        self,
        method_node: ast.FunctionDef,
        literal_to_call: dict[str, CallInfo],
        relative_to: Path | None = None,
    ) -> list[str]:
        """Unparse a method definition with decorators and proper formatting."""
        lines: list[str] = []

        # Decorators
        for dec in method_node.decorator_list:
            lines.append(f"    @{self.unparse_expr(dec)}")

        # Method signature
        args_parts: list[str] = []
        for arg in method_node.args.args:
            if arg.annotation:
                ann_str = self.unparse_expr(arg.annotation)
                args_parts.append(f"{arg.arg}: {ann_str}")
            else:
                args_parts.append(arg.arg)

        # Format type parameters if present
        type_params_str = ""
        if hasattr(method_node, "type_params") and method_node.type_params:
            type_param_names = [
                tp.name if isinstance(tp, ast.TypeVar) else str(tp)
                for tp in method_node.type_params
            ]
            type_params_str = f"[{', '.join(type_param_names)}]"

        # Return type
        returns_str = ""
        if method_node.returns:
            returns_str = f" -> {self.unparse_expr(method_node.returns)}"

        lines.append(f"    def {method_node.name}{type_params_str}(")
        for i, arg_str in enumerate(args_parts):
            if i < len(args_parts) - 1:
                lines.append(f"        {arg_str},")
            else:
                lines.append(f"        {arg_str},")
        lines.append(f"    ){returns_str}:")

        # Add source location comment if this is a Literal overload
        literal_value = self.extract_literal_from_method(method_node)
        if literal_value and literal_value in literal_to_call:
            call = literal_to_call[literal_value]
            lineno = call.get("lineno")
            file_path = call.get("file")
            if lineno and file_path:
                from pathlib import Path as _P

                module_name = _P(file_path).stem
                lines.append(f"        # from {module_name}:{lineno}")

        # Body
        lines.append("        ...")
        lines.append("    ")

        return lines

    def extract_literal_from_method(self, method_node: ast.FunctionDef) -> str | None:
        """Extract the Literal string value from a method's query parameter annotation."""
        if not method_node.args.args:
            return None

        # Look for the query parameter (should be the last one)
        for arg in method_node.args.args:
            if arg.arg == "query" and arg.annotation:
                if isinstance(arg.annotation, ast.Subscript):
                    if (
                        isinstance(arg.annotation.value, ast.Name)
                        and arg.annotation.value.id == "Literal"
                    ):
                        if isinstance(arg.annotation.slice, ast.Constant):
                            return str(arg.annotation.slice.value)
        return None

    def unparse_expr(self, node: ast.expr) -> str:
        """Unparse an expression node to a string.

        Handles special cases for Literal with double quotes.
        """
        if isinstance(node, ast.Name):
            return node.id
        elif isinstance(node, ast.Constant):
            # Use double quotes for strings
            if isinstance(node.value, str):
                if "\n" in node.value:
                    return '"""' + node.value + '"""'
                else:
                    # Escape any double quotes in the string
                    escaped = node.value.replace('"', '\\"')
                    return f'"{escaped}"'
            return repr(node.value)
        elif isinstance(node, ast.Subscript):
            value_str = self.unparse_expr(node.value)
            slice_str = self.unparse_expr(node.slice)
            return f"{value_str}[{slice_str}]"
        elif isinstance(node, ast.Attribute):
            value_str = self.unparse_expr(node.value)
            return f"{value_str}.{node.attr}"
        elif isinstance(node, ast.Tuple):
            # In type annotation context, Tuple just represents comma-separated elements
            # Don't wrap with "tuple[...]" - that's already in the parent Subscript
            elements = [self.unparse_expr(elt) for elt in node.elts]
            return f"{', '.join(elements)}"
        elif isinstance(node, ast.List):
            elements = [self.unparse_expr(elt) for elt in node.elts]
            return f"[{', '.join(elements)}]"
        else:
            # Fallback to ast.unparse for complex expressions
            return ast.unparse(node)

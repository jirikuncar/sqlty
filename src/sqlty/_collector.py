"""AST visitor for collecting SQL function calls."""

import ast
import logging
from typing import TypedDict


class CallInfo(TypedDict):
    """Information about a function call with literal argument."""

    literal: str
    return_type: str | None
    lineno: int
    file: str
    registry_source: str | None  # Track which module/registry this call belongs to
    registry_class: str | None  # Track which specific registry class (e.g., "RegistryA")


class LiteralCallCollector(ast.NodeVisitor):
    """Collects function calls with literal string arguments."""

    def __init__(self) -> None:
        self.function_name = "sql"  # Always look for .sql() method calls
        self.calls: list[CallInfo] = []
        self.current_assignment: ast.AnnAssign | ast.Assign | None = None
        self.import_sources: dict[
            str, str
        ] = {}  # Map import name -> module (e.g., {"user_sql": "user_queries"})
        self.module_imports: dict[
            str, str
        ] = {}  # Track module imports (e.g., import queries -> {"queries": "queries"})
        self.class_imports: dict[
            str, str
        ] = {}  # Track class imports with aliases (e.g., {"A": "QueryA"})

    def visit_Import(self, node: ast.Import) -> None:
        """Track module imports (e.g., import queries)."""
        for alias in node.names:
            module_name = alias.name
            import_as = alias.asname if alias.asname else alias.name
            self.module_imports[import_as] = module_name
        self.generic_visit(node)

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        """Track where functions are imported from."""
        if node.module:
            for alias in node.names:
                imported_name = alias.name
                local_name = alias.asname if alias.asname else alias.name
                # Track all imports, not just the target function
                self.import_sources[local_name] = node.module
                # Also track the original class name if it's different from the local name
                if alias.asname:
                    self.class_imports[local_name] = imported_name
        self.generic_visit(node)

    def visit_Assign(self, node: ast.Assign) -> None:
        """Visit assignment nodes to capture type context."""
        old_assignment = self.current_assignment
        self.current_assignment = node
        self.generic_visit(node)
        self.current_assignment = old_assignment

    def visit_AnnAssign(self, node: ast.AnnAssign) -> None:
        """Visit annotated assignment nodes."""
        old_assignment = self.current_assignment
        self.current_assignment = node
        self.generic_visit(node)
        self.current_assignment = old_assignment

    def visit_Call(self, node: ast.Call) -> None:
        """Visit function calls to find target function with literal arguments."""
        # Check for direct call: sql(...) or user_sql(...) etc.
        registry_source: str | None = None
        registry_class: str | None = None
        is_target_call = False

        if isinstance(node.func, ast.Name):
            func_name = node.func.id
            # Check if it's a call to the target function or an alias/import of it
            if func_name == self.function_name or func_name.endswith(f"_{self.function_name}"):
                is_target_call = True
                # Check if it was imported from somewhere
                if func_name in self.import_sources:
                    registry_source = self.import_sources[func_name]
                # If not imported, still mark as valid (no registry_source)
            elif func_name in self.import_sources:
                # This is an imported/aliased name (e.g., sql1 from "import sql as sql1")
                # Check if it's an alias of sql by looking at class_imports
                if (
                    func_name in self.class_imports
                    and self.class_imports[func_name] == self.function_name
                ):
                    is_target_call = True
                    registry_source = self.import_sources[func_name]

        # Check for qualified call: module.sql(...) or RegistryClass.sql(...)
        if isinstance(node.func, ast.Attribute) and node.func.attr == self.function_name:
            if isinstance(node.func.value, ast.Name):
                module_or_class_name = node.func.value.id
                # Priority 1: Check if it's a module import (e.g., queries.sql)
                if module_or_class_name in self.module_imports:
                    registry_source = self.module_imports[module_or_class_name]
                    is_target_call = True
                # Priority 2: Check if it's a class imported from somewhere (e.g., QueryRegistry.sql or A.sql where A is an alias)
                elif module_or_class_name in self.import_sources:
                    registry_source = self.import_sources[module_or_class_name]
                    # Check if this is an aliased import and get the original class name
                    if module_or_class_name in self.class_imports:
                        registry_class = self.class_imports[module_or_class_name]
                    else:
                        # Not aliased, use the name as-is
                        registry_class = module_or_class_name
                    is_target_call = True
                else:
                    # This might be a direct registry class reference in the same file (e.g., RegistryA.sql())
                    # We'll mark it as a target call and capture the class name
                    registry_class = module_or_class_name
                    is_target_call = True

        # Add any direct or qualified call to the target function with a literal argument
        if is_target_call:
            if (
                node.args
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)
            ):
                literal_value = node.args[0].value
                return_type = self._infer_return_type(node)
                call_info: CallInfo = {
                    "literal": literal_value,
                    "return_type": return_type,
                    "lineno": node.lineno,
                    "file": "",
                    "registry_source": registry_source,
                    "registry_class": registry_class,
                }
                self.calls.append(call_info)
            elif node.args:
                logging.getLogger(__name__).warning(
                    "Call to %s() at line %s uses a non-literal argument. Type inference requires literal strings.",
                    self.function_name,
                    node.lineno,
                )

        self.generic_visit(node)

    def _infer_return_type(self, call_node: ast.Call) -> str | None:
        """Infer return type from assignment or annotation context."""
        if isinstance(self.current_assignment, ast.AnnAssign):
            # Get type from annotation
            return ast.unparse(self.current_assignment.annotation)
        elif isinstance(self.current_assignment, ast.Assign):
            # Try to find type from usage
            return self._infer_from_usage(call_node)
        return None

    def _infer_from_usage(self, call_node: ast.Call) -> str | None:
        """Infer type from how the result is used (simplified)."""
        # This is a placeholder - in a full implementation, we'd do data flow analysis
        return None

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
    """Collects function calls with literal string arguments.
    
    Only collects calls that are verified to come from SQLRegistry subclasses.
    """

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
        self.local_registry_classes: set[
            str
        ] = set()  # Track SQLRegistry subclasses defined in current file
        self.registry_aliases: dict[
            str, str
        ] = {}  # Track aliases to .sql methods (e.g., {"user_sql": "UserQueries"})

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
                else:
                    # Even without alias, track the original name
                    self.class_imports[local_name] = imported_name
        self.generic_visit(node)

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        """Track SQLRegistry subclasses defined in the current file."""
        # Check if this class inherits from SQLRegistry
        for base in node.bases:
            if isinstance(base, ast.Name) and base.id == "SQLRegistry":
                self.local_registry_classes.add(node.name)
                break
            elif isinstance(base, ast.Attribute) and base.attr == "SQLRegistry":
                self.local_registry_classes.add(node.name)
                break
        self.generic_visit(node)

    def visit_Assign(self, node: ast.Assign) -> None:
        """Visit assignment nodes to capture type context and track .sql aliases."""
        # Check if this is an assignment like: user_sql = UserQueries.sql
        if (
            isinstance(node.value, ast.Attribute)
            and node.value.attr == self.function_name
            and isinstance(node.value.value, ast.Name)
        ):
            class_name = node.value.value.id
            # Check if it's a local registry class
            if class_name in self.local_registry_classes:
                # Track the alias
                for target in node.targets:
                    if isinstance(target, ast.Name):
                        self.registry_aliases[target.id] = class_name
        
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
        """Visit function calls to find target function with literal arguments.
        
        Only accepts calls that are verified to come from SQLRegistry subclasses.
        """
        # Check for direct call: sql(...) or user_sql(...) etc.
        registry_source: str | None = None
        registry_class: str | None = None
        is_target_call = False

        if isinstance(node.func, ast.Name):
            func_name = node.func.id
            
            # Check if this is an alias to a local registry's .sql method
            if func_name in self.registry_aliases:
                is_target_call = True
                registry_class = self.registry_aliases[func_name]
                # registry_source will be set later by the generator based on the current file
            # Check if it's imported from another module
            elif func_name in self.import_sources:
                is_target_call = True
                registry_source = self.import_sources[func_name]
                # Also check if we know the original class name
                if func_name in self.class_imports:
                    registry_class = self.class_imports[func_name]

        # Check for qualified call: module.sql(...) or RegistryClass.sql(...)
        elif isinstance(node.func, ast.Attribute) and node.func.attr == self.function_name:
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
                # Priority 3: Check if it's a local SQLRegistry class (e.g., RegistryA.sql())
                elif module_or_class_name in self.local_registry_classes:
                    registry_class = module_or_class_name
                    is_target_call = True
                    # registry_source will be set later by the generator based on the current file

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

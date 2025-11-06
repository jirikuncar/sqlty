"""AST visitor for collecting SQL function calls."""

import ast
import logging
from typing import TypedDict

import asttokens


class CallInfo(TypedDict):
    """Information about a function call with literal argument."""

    literal: str
    return_type: str | None
    lineno: int
    file: str
    registry_source: str | None  # Track which module/registry this call belongs to
    registry_class: str | None  # Track which specific registry class (e.g., "RegistryA")
    start_offset: int | None  # Starting byte offset in source file
    end_offset: int | None  # Ending byte offset in source file
    start_col: int | None  # Starting column number (0-indexed)
    end_line: int | None  # Ending line number (1-indexed)
    end_col: int | None  # Ending column number (0-indexed)


class LiteralCallCollector(ast.NodeVisitor):
    """Collects function calls with literal string arguments.

    Only collects calls that are verified to come from SQLRegistry subclasses.
    """

    def __init__(self, atok: asttokens.ASTTokens | None = None) -> None:
        self.function_name = "sql"  # Always look for .sql() method calls
        self.calls: list[CallInfo] = []
        self.current_assignment: ast.AnnAssign | ast.Assign | None = None
        self.current_file: str = ""
        self.import_sources: dict[
            str, str
        ] = {}  # Map import name -> module (e.g., {"user_sql": "user_queries"})
        self.import_levels: dict[
            str, int
        ] = {}  # Track relative import levels (e.g., {"sql": 2} for from .. import sql)
        self.module_imports: dict[
            str, str
        ] = {}  # Track module imports (e.g., import queries -> {"queries": "queries"})
        self.class_imports: dict[
            str, str
        ] = {}  # Track class imports with aliases (e.g., {"A": "QueryA"})
        self.local_registry_classes: set[str] = (
            set()
        )  # Track SQLRegistry subclasses defined in current file
        self.registry_aliases: dict[
            str, str
        ] = {}  # Track aliases to .sql methods (e.g., {"user_sql": "UserQueries"})
        self.decorated_functions: set[str] = set()  # Track functions decorated with @sqlty
        self.atok = atok  # ASTTokens for source range extraction

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

    def visit_Import(self, node: ast.Import) -> None:
        """Track module imports (e.g., import queries)."""
        for alias in node.names:
            module_name = alias.name
            import_as = alias.asname if alias.asname else alias.name
            self.module_imports[import_as] = module_name
        self.generic_visit(node)

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        """Track where functions are imported from."""
        # Handle relative imports: node.level > 0 means relative import
        # node.module can be None for imports like "from .. import sql"
        module_name = node.module if node.module else ""
        import_level = node.level if node.level else 0

        for alias in node.names:
            imported_name = alias.name
            local_name = alias.asname if alias.asname else alias.name
            # Track all imports, not just the target function
            self.import_sources[local_name] = module_name
            self.import_levels[local_name] = import_level
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
            if (
                isinstance(base, ast.Name)
                and base.id == "SQLRegistry"
                or isinstance(base, ast.Attribute)
                and base.attr == "SQLRegistry"
            ):
                self.local_registry_classes.add(node.name)
                break
        self.generic_visit(node)

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        """Track functions decorated with @sqlty."""
        # Check if the function has @sqlty decorator
        for decorator in node.decorator_list:
            # Handle @sqlty (simple name)
            if isinstance(decorator, ast.Name) and decorator.id == "sqlty":
                self.decorated_functions.add(node.name)
                break
            # Handle @sqlty(...) (call expression)
            elif isinstance(decorator, ast.Call):
                if isinstance(decorator.func, ast.Name) and decorator.func.id == "sqlty":
                    self.decorated_functions.add(node.name)
                    break
        self.generic_visit(node)

    def visit_Assign(self, node: ast.Assign) -> None:
        """Visit assignment nodes to capture type context and track .sql aliases."""
        # TODO remove ???
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

            # Priority 1: Check if this is a decorated function with @sqlty
            if func_name in self.decorated_functions:
                is_target_call = True
                registry_class = func_name  # Use function name as registry identifier
                # registry_source will be set later by the generator based on the current file
            # Priority 2: Check if it's imported from another module
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

        # Add any direct or qualified call to the target function with a literal argument
        if is_target_call:
            if (
                node.args
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)
            ):
                literal_value = node.args[0].value
                return_type = self._infer_return_type(node)

                # Extract source range information
                start_offset = None
                end_offset = None
                start_col = None
                end_line = None
                end_col = None

                if self.atok:
                    try:
                        start_offset, end_offset = self.atok.get_text_range(node)
                        # Convert offsets to (line, col) tuples
                        # Note: We use node.lineno for start_line (already 1-indexed from AST)
                        # We only need start_col here, as node.lineno is already the start line
                        _, start_col = self._offset_to_line(start_offset)
                        end_line, end_col = self._offset_to_line(end_offset)
                    except Exception as e:
                        logging.getLogger(__name__).debug("Failed to extract source range: %s", e)

                call_info: CallInfo = {
                    "literal": literal_value,
                    "return_type": return_type,
                    "lineno": node.lineno,  # This is start_line (1-indexed from AST)
                    "file": "",
                    "registry_source": registry_source,
                    "registry_class": registry_class,
                    "start_offset": start_offset,
                    "end_offset": end_offset,
                    "start_col": start_col,
                    "end_line": end_line,
                    "end_col": end_col,
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

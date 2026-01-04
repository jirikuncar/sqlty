"""AST visitor for collecting SQL function calls."""

import ast
import logging
from typing import TYPE_CHECKING, TypedDict

import asttokens

if TYPE_CHECKING:
    from sqlty._config import FunctionConfigComplete


class CallInfo(TypedDict, total=False):
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
    # NEW: Fields for tracking configured functions with multiple parameters
    query_param_name: str | None  # Name of the query parameter (e.g., "query")
    return_type_param_name: str | None  # Name of the return type parameter (e.g., "return_tp")
    return_type_param_value: str | None  # Actual value passed to return_type param (e.g., "int")
    schema_path: str | None  # Path to SQL schema file for type inference (from function config)
    function_name: str | None  # Name of the function being called (for configured functions)
    # Template fields for custom type generation
    return_type_template: str | None  # Template for return type with {sqlty} placeholder
    self_type_template: str | None  # Template for self parameter type with {sqlty} placeholder
    imports: list[str] | None  # List of import statements needed for the stub


class LiteralCallCollector(ast.NodeVisitor):
    """Collects function calls with literal string arguments.

    Only collects calls from functions explicitly configured in pyproject.toml.
    Auto-detection of @sqlty decorators has been removed - functions must be
    registered in the configuration file.
    """

    def __init__(
        self,
        atok: asttokens.ASTTokens | None = None,
        function_configs: list["FunctionConfigComplete"] | None = None,
    ) -> None:
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
        ] = {}  # Track class/function imports with aliases (e.g., {"A": "QueryA"})
        self.atok = atok  # ASTTokens for source range extraction

        # NEW: Support for configured functions
        self.function_configs = function_configs or []
        # Build a mapping for quick lookup: (module.function) -> config
        # Using full import_path as key to handle multiple functions with same name in different modules
        self.configured_functions: dict[str, FunctionConfigComplete] = {}
        # Track which function names are registered locally (for local calls)
        # This will be populated after current_file is set
        self.registered_function_names: set[str] = set()
        for config in self.function_configs:
            # Use full import_path as key to avoid collisions when multiple modules have same function name
            import_path = config["import_path"]
            self.configured_functions[import_path] = config

    def set_current_file_registrations(self, filepath: str) -> None:
        """Populate registered_function_names for the current file.

        Args:
            filepath: Absolute path to the file being analyzed
        """
        from pathlib import Path

        # Extract module name from filepath (e.g., /path/to/app.py -> app)
        module_name = Path(filepath).stem

        # Find all registered functions that belong to this module
        for config in self.function_configs:
            import_path = config["import_path"]
            # import_path format: "module.function" or "package.module.function"
            parts = import_path.split(".")
            if len(parts) >= 2:
                # Check if the module part matches our file
                config_module = parts[-2]  # Second to last is the module
                if config_module == module_name:
                    func_name = parts[-1]
                    self.registered_function_names.add(func_name)

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

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        """Visit function definitions (no longer tracks @sqlty decorators)."""
        # Auto-detection of @sqlty decorators has been removed
        # Functions must be explicitly registered in pyproject.toml
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

        Only accepts calls that are verified to come from @sqlty decorated functions
        or configured functions.
        """
        # NEW: Check if this is a configured function call FIRST
        # This takes priority over @sqlty decorated functions
        func_name_to_check = None
        if isinstance(node.func, ast.Name):
            func_name_to_check = node.func.id
        elif isinstance(node.func, ast.Attribute):
            func_name_to_check = node.func.attr

        config = None

        # Priority 1: Check if this is a locally-registered function (defined in current file)
        if func_name_to_check and func_name_to_check in self.registered_function_names:
            # Look up the config for this local function.
            # IMPORTANT: We must match both the function name AND the module name.
            # This is crucial for multi-registry patterns where multiple files define
            # functions with the same name (e.g., both user_queries.py and post_queries.py
            # define sql()). Without matching the module, we'd incorrectly pick the first
            # config entry for all files, causing stubs to be incorrectly combined.
            from pathlib import Path

            current_module = Path(self.current_file).stem if self.current_file else None
            for potential_config in self.function_configs:
                import_path_parts = potential_config["import_path"].split(".")
                if len(import_path_parts) >= 2:
                    config_module = import_path_parts[-2]  # Module name (e.g., "user_queries")
                    config_func = import_path_parts[-1]  # Function name (e.g., "sql")
                    # Match both module and function name to ensure correct config selection
                    if config_func == func_name_to_check and config_module == current_module:
                        config = potential_config
                        break
                else:
                    # Fallback for simple import paths (just function name)
                    func_name = import_path_parts[-1]
                    if func_name == func_name_to_check:
                        config = potential_config
                        break
        # Priority 2: Check if func_name_to_check is an imported alias for a configured function
        elif func_name_to_check and func_name_to_check in self.import_sources:
            imported_module = self.import_sources[func_name_to_check]
            import_level = self.import_levels.get(func_name_to_check, 0)

            # Get the original name if this is an alias
            original_func_name = self.class_imports.get(func_name_to_check, func_name_to_check)

            # For relative imports, we need to resolve the module name
            if import_level > 0:
                # Resolve relative import to absolute module path
                # We need to build the full module path from the current file's location
                from pathlib import Path

                # Get current file's package path
                if self.current_file:
                    current_path = Path(self.current_file)
                    # Build the resolved absolute module path
                    # Start from current file's parent and go up (level-1) times
                    target_parts: list[str] = []

                    # Navigate up to find the package root by walking up until we find a directory without __init__.py
                    check_dir = current_path.parent
                    while (check_dir / "__init__.py").exists():
                        target_parts.insert(0, check_dir.name)
                        check_dir = check_dir.parent

                    # Now navigate up based on import level
                    # Level 1 (.) = current package directory
                    # Level 2 (..) = parent package directory
                    # Remove (level - 1) parts from the end
                    if import_level <= len(target_parts):
                        target_parts = (
                            target_parts[: -(import_level - 1)]
                            if import_level > 1
                            else target_parts
                        )
                    else:
                        # Can't navigate up that far, skip
                        pass

                    # Add the imported module parts
                    if imported_module:
                        target_parts.extend(imported_module.split("."))

                    # Build the resolved absolute module path
                    resolved_module_path = ".".join(target_parts) if target_parts else None

                    # Match against configured functions
                    if resolved_module_path:
                        for potential_config in self.function_configs:
                            import_path = potential_config["import_path"]
                            parts = import_path.split(".")
                            if len(parts) >= 2:
                                config_func = parts[-1]
                                config_module_path = ".".join(parts[:-1])

                                # Check if the function name matches
                                if config_func == original_func_name:
                                    # Try exact match first
                                    if config_module_path == resolved_module_path:
                                        config = potential_config
                                        break
                                    # Fallback: for relative imports within a package, the resolved path
                                    # may include the package name (e.g., "multi_registry.user_queries")
                                    # while the config may only have the module name (e.g., "user_queries")
                                    # Check if resolved path ENDS WITH config path
                                    if (
                                        resolved_module_path.endswith("." + config_module_path)
                                        or resolved_module_path == config_module_path
                                    ):
                                        config = potential_config
                                        break
            else:
                # For absolute imports, direct matching
                for potential_config in self.function_configs:
                    import_path = potential_config["import_path"]
                    parts = import_path.split(".")
                    if len(parts) >= 2:
                        # Extract module and function from config's import_path
                        config_func = parts[-1]
                        config_module_path = ".".join(parts[:-1])

                        # Check if the function name matches and full module path matches
                        if (
                            config_module_path == imported_module
                            and config_func == original_func_name
                        ):
                            config = potential_config
                            break

        if config:
            # Find the query argument
            query_value, is_query_literal = self._extract_param_value(
                node, config["query_param"], config
            )

            # Only proceed if we have a literal SQL query
            if query_value and is_query_literal:
                # Find the return type argument (if specified in config)
                return_type_value = None
                return_type_param_name = config.get("return_type_param")

                if return_type_param_name:
                    return_type_value, _ = self._extract_param_value(
                        node, return_type_param_name, config
                    )

                # Extract source range information
                start_offset = None
                end_offset = None
                start_col = None
                end_line = None
                end_col = None

                if self.atok:
                    try:
                        start_offset, end_offset = self.atok.get_text_range(node)
                        _, start_col = self._offset_to_line(start_offset)
                        end_line, end_col = self._offset_to_line(end_offset)
                    except Exception as e:
                        logging.getLogger(__name__).debug("Failed to extract source range: %s", e)

                # Parse import_path to extract registry_source and registry_class
                # Format: "module.function"
                # TODO: Add support for class methods (e.g., "module.Class.method")
                # This will require:
                # 1. Parsing class.method from import_path
                # 2. Detecting method calls on instances (obj.method(...))
                # 3. Type inference for self parameter from instance type
                # 4. Generating proper stub structure with class definitions
                import_path_parts = config["import_path"].rsplit(".", 1)
                if len(import_path_parts) == 2:
                    config_registry_source = import_path_parts[0]
                    config_registry_class = import_path_parts[
                        1
                    ]  # Use the real function name from config
                else:
                    # Fallback
                    config_registry_source = config["import_path"]
                    config_registry_class = func_name_to_check or config["import_path"]

                call_info: CallInfo = {
                    "literal": query_value,
                    "return_type": return_type_value,  # This may be used by generator
                    "lineno": node.lineno,
                    "file": "",
                    "registry_source": config_registry_source,
                    "registry_class": config_registry_class,
                    "start_offset": start_offset,
                    "end_offset": end_offset,
                    "start_col": start_col,
                    "end_line": end_line,
                    "end_col": end_col,
                    "query_param_name": config["query_param"],
                    "return_type_param_name": return_type_param_name,
                    "return_type_param_value": return_type_value,
                    "schema_path": config.get("schema"),  # Optional schema path from config
                    "function_name": func_name_to_check,  # Store the function name
                    "return_type_template": config.get(
                        "return_type_template"
                    ),  # Template for return type
                    "self_type_template": config.get(
                        "self_type_template"
                    ),  # Template for self parameter
                    "imports": config.get("imports"),  # Custom imports for the stub
                }
                self.calls.append(call_info)

            # Early return - don't check for @sqlty decorated functions
            self.generic_visit(node)
            return

        # Check for direct call: sql(...) or user_sql(...) etc.
        # Note: Auto-detection of @sqlty decorators has been removed.
        # Only functions registered in pyproject.toml or imported will be tracked.
        registry_source: str | None = None
        registry_class: str | None = None
        is_target_call = False

        if isinstance(node.func, ast.Name):
            func_name = node.func.id

            # Priority 1: Check if it's imported from another module (registered in pyproject.toml)
            if func_name in self.import_sources:
                is_target_call = True
                registry_source = self.import_sources[func_name]
                # Also check if we know the original class name
                if func_name in self.class_imports:
                    registry_class = self.class_imports[func_name]
            # Priority 2: Check if it's a locally-defined registered function
            elif func_name in self.registered_function_names:
                is_target_call = True
                registry_class = func_name
                # registry_source will be determined by the generator based on the current file

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

                sql_call_info: CallInfo = {
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
                    "query_param_name": None,
                    "return_type_param_name": None,
                    "return_type_param_value": None,
                    "schema_path": None,  # Schema not set for @sqlty decorated functions (uses __schema_path__ attribute)
                    "function_name": None,  # Not applicable for @sqlty decorated functions
                }
                self.calls.append(sql_call_info)
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

    def _extract_param_value(
        self, node: ast.Call, param_name: str, config: "FunctionConfigComplete"
    ) -> tuple[str | None, bool]:
        """Extract the value of a parameter from a function call.

        Args:
            node: The function call AST node
            param_name: The name of the parameter to extract
            config: The function configuration

        Returns:
            Tuple of (value, is_literal) where:
            - value: The extracted value as a string (None if not found or not extractable)
            - is_literal: True if the value is a literal string, False if it's a type or expression
        """
        # Check keyword arguments first
        for keyword in node.keywords:
            if keyword.arg == param_name:
                if isinstance(keyword.value, ast.Constant):
                    # String literals
                    if isinstance(keyword.value.value, str):
                        return keyword.value.value, True
                    # Other constants (numbers, bool, None)
                    return str(keyword.value.value), False
                elif isinstance(keyword.value, ast.Name):
                    # Type references like `return_tp=int` or variable names
                    return keyword.value.id, False
                elif isinstance(keyword.value, ast.Attribute):
                    # Qualified names like `return_tp=module.Type`
                    return ast.unparse(keyword.value), False
                elif isinstance(keyword.value, ast.Subscript):
                    # Generic types like `return_tp=list[int]`
                    return ast.unparse(keyword.value), False
                else:
                    # Try to unparse for more complex expressions
                    try:
                        return ast.unparse(keyword.value), False
                    except Exception:
                        return None, False

        # Try to extract from positional arguments
        # For methods, the signature is typically: def method(self, param1, param2, ...)
        # So positional arg 0 maps to param1 (first param after self)
        # For functions, positional arg 0 maps to param0 (first param)

        # Get the parameter name we're looking for
        # For query_param: support it as the first positional argument
        # OR if there's only one string literal argument, use that automatically
        if param_name == config["query_param"]:
            # If there's exactly one positional argument and it's a string literal,
            # use it automatically (automatic detection)
            if len(node.args) == 1 or len(node.args) > 0:
                arg = node.args[0]
                if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                    return arg.value, True

        # For return_type_param, it's typically not a positional argument
        # but we could support it if needed in the future

        return None, False

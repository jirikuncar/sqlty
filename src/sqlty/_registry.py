"""Registry detection and schema resolution utilities.

This module handles detecting @sqlty decorated functions
in Python modules and resolving their associated schema files.
"""

import ast
import logging
from pathlib import Path

from sqlty import sqlty

logger = logging.getLogger(__name__)


class RegistryDetector:
    """Detect @sqlty decorated functions and resolve their schemas."""

    def get_schema_path_from_registry(
        self, module_path: Path, name: str | None = None
    ) -> Path | None:
        """Extract __schema_path__ from a @sqlty decorated function.

        Args:
            module_path: Path to the Python module to check
            name: Optional specific function name to look for.

        Returns:
            Resolved schema path if found, None otherwise
        """
        if not module_path.exists():
            return None

        try:
            source = module_path.read_text()
            tree = ast.parse(source)
            for node in ast.walk(tree):
                # Check decorated functions with @sqlty(schema="...")
                if isinstance(node, ast.FunctionDef):
                    if name and node.name != name:
                        continue

                    # Check if function has @sqlty decorator
                    for decorator in node.decorator_list:
                        # Handle @sqlty(schema="...")
                        if isinstance(decorator, ast.Call):
                            if (
                                isinstance(decorator.func, ast.Name)
                                and decorator.func.id == sqlty.__name__
                            ):
                                # TODO check the source module of the sqlty function
                                # Look for schema keyword argument
                                for keyword in decorator.keywords:
                                    if (
                                        keyword.arg == "schema"
                                        and isinstance(keyword.value, ast.Constant)
                                        and isinstance(keyword.value.value, str)
                                    ):
                                        schema_path = module_path.parent / keyword.value.value
                                        return schema_path
        except Exception:
            pass
        return None

    def get_registry_names(self, module_path: Path) -> list[str]:
        """Get the names of all @sqlty decorated functions in a module.

        Args:
            module_path: Path to the Python module to check

        Returns:
            List of detected registry names
        """
        if not module_path.exists():
            return []

        registry_classes: list[str] = []
        try:
            with open(module_path) as f:
                source = f.read()
            tree = ast.parse(source)

            # Always include functions decorated with @sqlty
            for node in ast.walk(tree):
                if isinstance(node, ast.FunctionDef):
                    for decorator in node.decorator_list:
                        if (isinstance(decorator, ast.Name) and decorator.id == "sqlty") or (
                            isinstance(decorator, ast.Call)
                            and isinstance(decorator.func, ast.Name)
                            and decorator.func.id == "sqlty"
                        ):
                            if node.name not in registry_classes:
                                registry_classes.append(node.name)
                            break
        except Exception:
            logger.debug("Failed to parse for decorated functions: %s", module_path)

        return registry_classes

    def get_registry_export_name(self, module_path: Path, function_name: str) -> str:
        """Get the exported name for the @sqlty decorated function.

        Args:
            module_path: Path to the Python module to check
            function_name: Name of the @sqlty decorated function

        Returns:
            The export name (the function name itself)
        """
        # For @sqlty decorated functions, the export name is the function name
        return function_name

    def get_class_name_from_export(self, module_path: Path, export_name: str) -> str | None:
        """Get the registry class name from an exported .sql method name.

        Args:
            module_path: Path to the Python module to check
            export_name: Name of the exported function (e.g., 'user_sql' or 'qa')

        Returns:
            The class name (e.g., 'UserQueries' or 'A'), or None if not found
        """
        if not module_path.exists():
            return None

        try:
            with open(module_path) as f:
                source = f.read()
            tree = ast.parse(source)

            # Look for assignments like: user_sql = UserQueries.sql or qa = A.sql
            for node in tree.body:
                if isinstance(node, ast.Assign):
                    # Check if the target matches the export_name
                    if (
                        node.targets
                        and isinstance(node.targets[0], ast.Name)
                        and node.targets[0].id == export_name
                    ):
                        # Check if the value is ClassName.sql
                        if isinstance(node.value, ast.Attribute):
                            if node.value.attr == "sql" and isinstance(node.value.value, ast.Name):
                                return node.value.value.id
            return None
        except Exception:
            return None

    def is_decorated_function(self, module_path: Path, name: str) -> bool:
        """Check if a name in a module is a decorated function rather than a class.

        Args:
            module_path: Path to the Python module to check
            name: Name to check (could be class or function name)

        Returns:
            True if name is a function decorated with @sqlty, False otherwise
        """
        if not module_path.exists():
            return False

        try:
            source = module_path.read_text()
            tree = ast.parse(source)
            for node in ast.walk(tree):
                if isinstance(node, ast.FunctionDef) and node.name == name:
                    # Check if it has @sqlty decorator
                    for decorator in node.decorator_list:
                        if isinstance(decorator, ast.Name) and decorator.id == "sqlty":
                            return True
                        elif isinstance(decorator, ast.Call):
                            if (
                                isinstance(decorator.func, ast.Name)
                                and decorator.func.id == "sqlty"
                            ):
                                return True
            return False
        except Exception:
            return False

    def resolve_import_to_registry_module(
        self, import_source: str, source_file: Path, import_level: int = 0
    ) -> str | None:
        """Resolve an import to find which module contains the @sqlty decorated function.

        Args:
            import_source: The module name from the import statement
            source_file: The file that contains the import
            import_level: The relative import level (0 for absolute, 1+ for relative)
                         Level 1 = . (current package)
                         Level 2 = .. (parent package)
                         Level 3 = ... (grandparent package), etc.

        Returns:
            The module name that contains the @sqlty decorated function, or None
        """
        logger.debug(
            "Resolving import: source='%s', file='%s', level=%d",
            import_source,
            source_file,
            import_level,
        )
        source_dir = source_file.parent

        # Handle relative imports
        if import_level > 0:
            # Navigate up the directory tree
            # Level 1 (.) = current directory (source_dir)
            # Level 2 (..) = parent directory (go up 1)
            # Level 3 (...) = grandparent directory (go up 2), etc.
            target_dir = source_dir
            levels_to_go_up = import_level - 1

            for _ in range(levels_to_go_up):
                target_dir = target_dir.parent
                if target_dir == target_dir.parent:  # Reached root
                    logger.debug("Reached root directory, cannot resolve relative import")
                    return None

            logger.debug(
                "Target directory for relative import (level %d): %s", import_level, target_dir
            )

            # If import_source is empty (e.g., "from .. import sql"), check target_dir
            if not import_source:
                # Check for __init__.py in target directory
                init_file = target_dir / "__init__.py"
                logger.debug("Checking __init__.py: %s", init_file)
                if self.get_registry_names(init_file):
                    # Return the module name based on the directory path
                    resolved = target_dir.name
                    logger.debug("Found registry in __init__.py, returning: %s", resolved)
                    return resolved
                logger.debug("No registry found in __init__.py")
                return None

            # Otherwise, resolve from target_dir
            module_path_parts = import_source.split(".")

            # Try as a file
            module_file = target_dir
            for part in module_path_parts[:-1]:
                module_file = module_file / part
            module_file = module_file / f"{module_path_parts[-1]}.py"

            logger.debug("Trying module file: %s", module_file)
            if self.get_registry_names(module_file):
                logger.debug("Found registry in module file, returning: %s", import_source)
                return import_source

            # Try as a package
            package_init = target_dir
            for part in module_path_parts:
                package_init = package_init / part
            package_init = package_init / "__init__.py"

            logger.debug("Trying package __init__: %s", package_init)
            if self.get_registry_names(package_init):
                logger.debug("Found registry in package __init__, returning: %s", import_source)
                return import_source

            logger.debug("Could not resolve relative import")
            return None

        # Handle absolute imports (original logic)
        module_path_parts = import_source.split(".")

        # Try as a simple relative import first (e.g., "queries" from same directory)
        module_file = source_dir / f"{import_source}.py"
        logger.debug("Trying simple module: %s", module_file)
        if self.get_registry_names(module_file):
            logger.debug("Found registry, returning: %s", import_source)
            return import_source

        # Try as a package with __init__.py
        package_init = source_dir / import_source / "__init__.py"
        logger.debug("Trying package: %s", package_init)
        if self.get_registry_names(package_init):
            logger.debug("Found registry in package, returning: %s", import_source)
            return import_source

        # For dotted imports, try multiple resolution strategies
        if len(module_path_parts) > 1:
            # Strategy 1: Resolve from current directory
            # e.g., from app.py: import myapp.queries.users
            # Try: <source_dir>/myapp/queries/users.py
            module_file = source_dir
            for part in module_path_parts[:-1]:
                module_file = module_file / part
            module_file = module_file / f"{module_path_parts[-1]}.py"

            if self.get_registry_names(module_file):
                return import_source

            # Strategy 2: Resolve as absolute import from parent directories
            # Walk up the directory tree to find the package root
            # For myapp/app.py with import myapp.queries.users:
            # Try: <parent_of_myapp>/myapp/queries/users.py
            current_dir = source_dir
            for _ in range(len(module_path_parts)):
                if current_dir.parent == current_dir:  # reached root
                    break
                current_dir = current_dir.parent

                # Try to resolve from this directory
                module_file = current_dir
                for part in module_path_parts[:-1]:
                    module_file = module_file / part
                module_file = module_file / f"{module_path_parts[-1]}.py"

                if self.get_registry_names(module_file):
                    return import_source

                # Also try as package
                package_init = current_dir
                for part in module_path_parts:
                    package_init = package_init / part
                package_init = package_init / "__init__.py"

                if self.get_registry_names(package_init):
                    return import_source

        # If not found, return None
        return None

    def find_registries(self, root_path: Path) -> list[tuple[Path, str]]:
        """Find all @sqlty decorated functions in Python files under the given path.

        Args:
            root_path: The root directory or file to search for Python files

        Returns:
            List of tuples containing (file_path, function_name)
        """
        registries: list[tuple[Path, str]] = []

        # Find all Python files
        python_files = [root_path] if root_path.is_file() else sorted(root_path.rglob("*.py"))

        for py_file in python_files:
            try:
                # Get all registry class names in this file
                class_names = self.get_registry_names(py_file)
                for class_name in class_names:
                    registries.append((py_file, class_name))
            except (SyntaxError, UnicodeDecodeError, OSError):
                # Skip files that can't be parsed or read
                continue

        return registries

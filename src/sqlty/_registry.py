"""Registry detection and schema resolution utilities.

This module handles detecting SQLRegistry subclasses in Python modules
and resolving their associated schema files.
"""

import ast
from pathlib import Path


class RegistryDetector:
    """Detect SQLRegistry subclasses and resolve their schemas."""

    def get_schema_path_from_registry(
        self, module_path: Path, class_name: str | None = None
    ) -> Path | None:
        """Extract __schema_path__ from a SQLRegistry subclass.

        Args:
            module_path: Path to the Python module to check
            class_name: Optional specific registry class name to look for.
                       If None, returns the first __schema_path__ found (backward compatible).

        Returns:
            Resolved schema path if found, None otherwise
        """
        if not module_path.exists():
            return None

        try:
            source = module_path.read_text()
            tree = ast.parse(source)
            for node in ast.walk(tree):
                if isinstance(node, ast.ClassDef):
                    # If class_name is specified, only check that specific class
                    if class_name and node.name != class_name:
                        continue

                    for base in node.bases:
                        if (isinstance(base, ast.Name) and base.id == "SQLRegistry") or (
                            isinstance(base, ast.Attribute) and base.attr == "SQLRegistry"
                        ):
                            # Look for __schema_path__ assignment
                            for stmt in node.body:
                                if (
                                    isinstance(stmt, ast.Assign)
                                    and len(stmt.targets) == 1
                                    and isinstance(stmt.targets[0], ast.Name)
                                    and stmt.targets[0].id == "__schema_path__"
                                    and isinstance(stmt.value, ast.Constant)
                                    and isinstance(stmt.value.value, str)
                                ):
                                    # Resolve path relative to module
                                    schema_path = module_path.parent / stmt.value.value
                                    return schema_path
        except Exception:
            pass
        return None

    def get_registry_class_names(self, module_path: Path) -> list[str]:
        """Get the names of all SQLRegistry subclasses in a module.

        Args:
            module_path: Path to the Python module to check

        Returns:
            List of SQLRegistry subclass names found in the module
        """
        if not module_path.exists():
            return []

        try:
            with open(module_path) as f:
                source = f.read()
            tree = ast.parse(source)

            # Look for class definitions that inherit from SQLRegistry
            registry_classes = []
            for node in ast.walk(tree):
                if isinstance(node, ast.ClassDef):
                    for base in node.bases:
                        # Check if base is Name node with id "SQLRegistry"
                        if isinstance(base, ast.Name) and base.id == "SQLRegistry":
                            registry_classes.append(node.name)
                            break
                        # Check if base is Attribute node like sqlty.SQLRegistry
                        if isinstance(base, ast.Attribute):
                            if base.attr == "SQLRegistry":
                                registry_classes.append(node.name)
                                break
            return registry_classes
        except Exception:
            return []

    def get_registry_export_name(self, module_path: Path, class_name: str) -> str:
        """Get the exported name for the registry's sql method.

        Args:
            module_path: Path to the Python module to check
            class_name: Name of the registry class

        Returns:
            The export name (e.g., 'user_sql'), or 'sql' as default
        """
        if not module_path.exists():
            return "sql"

        try:
            with open(module_path) as f:
                source = f.read()
            tree = ast.parse(source)

            # Look for assignments like: user_sql = UserQueries.sql
            # Iterate through top-level statements only
            for node in tree.body:
                if isinstance(node, ast.Assign):
                    # Check if the value is ClassName.sql
                    if isinstance(node.value, ast.Attribute):
                        if (
                            node.value.attr == "sql"
                            and isinstance(node.value.value, ast.Name)
                            and node.value.value.id == class_name
                        ):
                            # Found it! Get the target name
                            if node.targets and isinstance(node.targets[0], ast.Name):
                                return node.targets[0].id
            return "sql"
        except Exception:
            return "sql"

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
                            if (
                                node.value.attr == "sql"
                                and isinstance(node.value.value, ast.Name)
                            ):
                                return node.value.value.id
            return None
        except Exception:
            return None

    def resolve_import_to_registry_module(
        self, import_source: str, source_file: Path
    ) -> str | None:
        """Resolve an import to find which module contains the SQLRegistry subclass.

        Args:
            import_source: The module name from the import statement
            source_file: The file that contains the import

        Returns:
            The module name that contains the SQLRegistry subclass, or None
        """
        source_dir = source_file.parent
        module_path_parts = import_source.split(".")

        # Try as a simple relative import first (e.g., "queries" from same directory)
        module_file = source_dir / f"{import_source}.py"
        if self.get_registry_class_names(module_file):
            return import_source

        # Try as a package with __init__.py
        package_init = source_dir / import_source / "__init__.py"
        if self.get_registry_class_names(package_init):
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

            if self.get_registry_class_names(module_file):
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

                if self.get_registry_class_names(module_file):
                    return import_source

                # Also try as package
                package_init = current_dir
                for part in module_path_parts:
                    package_init = package_init / part
                package_init = package_init / "__init__.py"

                if self.get_registry_class_names(package_init):
                    return import_source

        # If not found, return None
        return None

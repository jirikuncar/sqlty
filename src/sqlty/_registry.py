"""Registry detection and schema resolution utilities.

This module handles detecting SQLRegistry subclasses in Python modules
and resolving their associated schema files.
"""

import ast
from pathlib import Path


class RegistryDetector:
    """Detect SQLRegistry subclasses and resolve their schemas."""

    def get_schema_path_from_registry(self, module_path: Path) -> Path | None:
        """Extract __schema_path__ from a SQLRegistry subclass.

        Args:
            module_path: Path to the Python module to check

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
                                    if schema_path.exists():
                                        return schema_path
        except Exception:
            pass
        return None

    def get_registry_class_name(self, module_path: Path) -> str | None:
        """Get the name of the SQLRegistry subclass in a module.

        Args:
            module_path: Path to the Python module to check

        Returns:
            The name of the first SQLRegistry subclass found, or None
        """
        if not module_path.exists():
            return None

        try:
            with open(module_path) as f:
                source = f.read()
            tree = ast.parse(source)

            # Look for class definitions that inherit from SQLRegistry
            for node in ast.walk(tree):
                if isinstance(node, ast.ClassDef):
                    for base in node.bases:
                        # Check if base is Name node with id "SQLRegistry"
                        if isinstance(base, ast.Name) and base.id == "SQLRegistry":
                            return node.name
                        # Check if base is Attribute node like sqlty.SQLRegistry
                        if isinstance(base, ast.Attribute):
                            if base.attr == "SQLRegistry":
                                return node.name
            return None
        except Exception:
            return None

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
        # Try to find the module file
        source_dir = source_file.parent
        module_file = source_dir / f"{import_source}.py"

        if self.get_registry_class_name(module_file):
            return import_source

        # Check if it's a package with __init__.py
        package_init = source_dir / import_source / "__init__.py"
        if self.get_registry_class_name(package_init):
            return import_source

        # If not found, return None
        return None

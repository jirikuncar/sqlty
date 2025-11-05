"""Command-line interface for discovering SQLRegistry subclasses.

Usage:
    python -m sqlty.registries [path]

Discovers and lists all SQLRegistry subclasses in the specified path (default: current directory).
Output format: <file_path>:<registry_name>
"""

import sys
from pathlib import Path

from sqlty._registry import RegistryDetector


def find_registries(root_path: Path) -> list[tuple[Path, str]]:
    """Find all SQLRegistry subclasses in Python files under the given path.

    Args:
        root_path: The root directory to search for Python files

    Returns:
        List of tuples containing (file_path, registry_class_name)
    """
    detector = RegistryDetector()
    registries: list[tuple[Path, str]] = []

    # Find all Python files
    python_files = [root_path] if root_path.is_file() else sorted(root_path.rglob("*.py"))

    for py_file in python_files:
        try:
            # Get all registry class names in this file
            class_names = detector.get_registry_class_names(py_file)
            for class_name in class_names:
                registries.append((py_file, class_name))
        except Exception:
            # Skip files that can't be parsed
            continue

    return registries


def main() -> None:
    """Main entry point for the registries command."""
    # Parse command-line arguments
    if len(sys.argv) > 2:
        print("Usage: python -m sqlty.registries [path]", file=sys.stderr)
        sys.exit(1)

    # Get the path argument (default to current directory)
    path_arg = sys.argv[1] if len(sys.argv) == 2 else "."
    search_path = Path(path_arg).resolve()

    # Validate the path
    if not search_path.exists():
        print(f"Error: Path '{path_arg}' does not exist", file=sys.stderr)
        sys.exit(1)

    # Find all registries
    registries = find_registries(search_path)

    # Print results
    for file_path, class_name in registries:
        print(f"{file_path}:{class_name}")


if __name__ == "__main__":
    main()

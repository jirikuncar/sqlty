"""Command-line interface for discovering SQLTy decorated functions.

Usage:
    python -m sqlty.registries [path]

Discovers and lists all @sqlty decorated functions (default: current directory).
Output format: <file_path>:<function_name>
"""

import sys
from pathlib import Path

from sqlty._registry import RegistryDetector


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

    # Print results
    for file_path, class_name in RegistryDetector().find_registries(search_path):
        print(f"{file_path}:{class_name}")

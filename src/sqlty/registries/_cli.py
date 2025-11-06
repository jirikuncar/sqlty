"""Command-line interface for discovering SQLRegistry subclasses.

Usage:
    python -m sqlty.registries [path]

Discovers and lists all SQLRegistry subclasses in the specified path (default: current directory).
Output format: <file_path>:<registry_name>
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

    # Find all registries
    detector = RegistryDetector()
    registries = detector.find_registries(search_path)

    # Print results
    for file_path, class_name in registries:
        print(f"{file_path}:{class_name}")

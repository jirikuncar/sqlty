"""Command-line interface for discovering registered SQLTy functions.

Usage:
    python -m sqlty.registries [path]

Discovers and lists all functions registered in pyproject.toml [tool.sqlty.functions].
This includes both @sqlty decorated functions and explicitly configured functions.
Searches from the given path (default: current directory).

Output format: <file_path>:<function_name>
"""

import sys
from pathlib import Path

from sqlty._config import load_config
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

    # Load configuration from pyproject.toml
    # Search from the search_path if it's a directory, otherwise from its parent
    config_search_path = search_path if search_path.is_dir() else search_path.parent
    config, _ = load_config(config_search_path)

    # Get function configs if available
    function_configs = config.get("functions", []) if config else []

    # Create registry detector with the loaded configuration
    detector = RegistryDetector(function_configs)

    # Print results
    for file_path, class_name in detector.find_registries(search_path):
        print(f"{file_path}:{class_name}")

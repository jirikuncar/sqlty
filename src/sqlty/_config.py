"""Configuration loading from pyproject.toml."""

import logging
from pathlib import Path
from typing import Any, TypedDict


class SQLTyConfig(TypedDict, total=False):
    """Configuration for SQLTy from pyproject.toml."""

    dialect: str
    schemas: dict[str, str]  # registry_name -> schema_path
    format_command: str  # e.g., "ruff format --stdin-filename {filename} -"


def load_config(project_root: Path | None = None) -> SQLTyConfig:
    """Load SQLTy configuration from pyproject.toml.

    Args:
        project_root: Root directory of the project. If None, searches from cwd upward.

    Returns:
        Configuration dictionary with SQLTy settings
    """
    if project_root is None:
        project_root = Path.cwd()

    # Search for pyproject.toml up the directory tree
    current = project_root.resolve()
    while current != current.parent:
        pyproject_path = current / "pyproject.toml"
        if pyproject_path.exists():
            try:
                return _parse_pyproject(pyproject_path)
            except Exception as e:
                logging.getLogger(__name__).warning("Failed to parse %s: %s", pyproject_path, e)
                return {}
        current = current.parent

    # No pyproject.toml found
    return {}


def _parse_pyproject(pyproject_path: Path) -> SQLTyConfig:
    """Parse pyproject.toml and extract [tool.sqlty] section.

    Args:
        pyproject_path: Path to pyproject.toml

    Returns:
        Configuration dictionary
    """
    try:
        import tomllib
    except ImportError:
        # Python < 3.11
        try:
            import tomli as tomllib  # type: ignore[import-not-found,no-redef]
        except ImportError:
            logging.getLogger(__name__).warning(
                "toml parsing not available. Install 'tomli' for Python < 3.11"
            )
            return {}

    with open(pyproject_path, "rb") as f:
        data: dict[str, Any] = tomllib.load(f)

    # Extract [tool.sqlty] section
    tool_config: dict[str, Any] = data.get("tool", {}).get("sqlty", {})

    config: SQLTyConfig = {}

    # Parse each configuration option
    if "dialect" in tool_config:
        config["dialect"] = str(tool_config["dialect"])

    if "schemas" in tool_config and isinstance(tool_config["schemas"], dict):
        # Convert all values to strings
        config["schemas"] = {k: str(v) for k, v in tool_config["schemas"].items()}

    # Allow hyphenated key in pyproject: format-command
    if "format-command" in tool_config:
        config["format_command"] = str(tool_config["format-command"])  # expects {filename}

    return config


def merge_config(config: SQLTyConfig, cli_args: dict[str, Any]) -> SQLTyConfig:
    """Merge configuration from pyproject.toml with CLI arguments.

    CLI arguments take precedence over config file.

    Args:
        config: Configuration from pyproject.toml
        cli_args: Configuration from CLI arguments (non-None values only)

    Returns:
        Merged configuration
    """
    merged: SQLTyConfig = config.copy()

    # CLI arguments override config file
    if cli_args.get("dialect") is not None:
        merged["dialect"] = str(cli_args["dialect"])
    if cli_args.get("format_command") is not None:
        merged["format_command"] = str(cli_args["format_command"])  # expects {filename}

    # For schemas, merge CLI schema mappings with config schemas
    if "schemas" in cli_args and cli_args["schemas"]:
        merged_schemas = config.get("schemas", {}).copy()
        merged_schemas.update(cli_args["schemas"])
        merged["schemas"] = merged_schemas

    return merged

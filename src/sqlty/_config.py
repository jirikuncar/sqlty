"""Configuration loading from pyproject.toml."""

import copy
import glob
import logging
from functools import lru_cache
from pathlib import Path
from typing import Any, TypedDict, cast

SchemaSpec = str | list[str]


class FunctionConfig(TypedDict, total=False):
    """Configuration for a specific function to track.

    Example:
        {
            "import_path": "db.pg_fetchvalue",
            "query_param": "query",
            "return_type_param": "return_tp",
            "schema": "path/to/schema.sql",
            "return_type_template": "list[{sqlty}]",
            "imports": ["from typing import Any"]
        }

    Note: Class methods (e.g., "module.Class.method") are not currently supported.
          Use module-level functions instead.
    """

    import_path: (
        str  # Full import path to the function, e.g., "module.submodule.function_name" (required)
    )
    query_param: str  # Name of the parameter containing the SQL query (required)
    return_type_param: str  # Name of the parameter specifying the return type (optional)
    schema: SchemaSpec  # Schema file, directory or glob, or a list of them (optional)
    return_type_template: str  # Template for return type with {sqlty} placeholder (optional)
    self_type_template: (
        str  # TODO: Template for self parameter - currently not supported for methods (optional)
    )
    imports: list[str]  # List of import statements needed for the stub (optional)


# Define required fields separately for type checking
class _FunctionConfigRequired(TypedDict):
    """Required fields for FunctionConfig."""

    import_path: str
    query_param: str


class FunctionConfigComplete(_FunctionConfigRequired, total=False):
    """Complete FunctionConfig with optional fields."""

    return_type_param: str
    schema: SchemaSpec
    return_type_template: str
    self_type_template: str
    imports: list[str]


class SQLTyConfig(TypedDict, total=False):
    """Configuration for SQLTy from pyproject.toml.

    Corresponds to the ``[tool.sqlty]`` section:

    .. code-block:: toml

        [tool.sqlty]
        dialect = "postgres"
        schema = "schema.sql"
        import-style = "from sqlty import SQL"
        untyped = "sentinel"

        [tool.sqlty.rules]
        select-star = "ignore"
        stale-type = "error"

        [[tool.sqlty.functions]]
        import_path = "db.query"
        query_param = "sql"
    """

    dialect: str
    schema: SchemaSpec  # Schema file, directory or glob, or a list of them
    schemas: dict[str, SchemaSpec]  # registry_name -> schema entries
    rules: dict[str, str]  # rule-name -> severity (error/warn/ignore)
    format_command: str  # e.g., "ruff format --stdin-filename {filename} -"
    import_style: str  # e.g., "from sqlty import SQL as Q" or "import sqlty"
    untyped: str  # "skip" (default) or "sentinel": SQL[UNTYPED] for uninferable queries
    functions: list[FunctionConfigComplete]  # List of functions to track and analyze


def _schema_spec(value: Any) -> SchemaSpec:
    if isinstance(value, list):
        return [str(v) for v in value]
    return str(value)


def expand_schema(spec: SchemaSpec | Path, base: Path) -> list[Path]:
    """List the schema files named by ``spec``, in order.

    ``spec`` is a path or a list of paths; each entry is a file, a directory
    (every ``*.sql`` under it, recursively) or a glob (``*``, ``?``, ``[``;
    ``**`` recurses), relative to ``base``. Directory and glob matches are sorted
    by path; repeated files are listed once. A plain path is kept even if it
    does not exist, so callers report it; a glob that matches nothing is logged.
    """
    entries = [spec] if isinstance(spec, (str, Path)) else spec
    files: dict[Path, None] = {}
    for entry in entries:
        path = base / entry
        if glob.has_magic(str(entry)):
            matches = [
                base / m for m in sorted(glob.glob(str(entry), root_dir=base, recursive=True))
            ]
            found = [m for m in matches if m.is_file()]
            if not found:
                logging.getLogger(__name__).warning("Schema pattern matches no files: %s", entry)
        elif path.is_dir():
            found = sorted(p for p in path.rglob("*.sql") if p.is_file())
        else:
            found = [path]
        files.update(dict.fromkeys(found))
    return list(files)


def load_config(project_root: Path | None = None) -> tuple[SQLTyConfig, Path | None]:
    """Load the ``[tool.sqlty]`` table of the nearest ``pyproject.toml`` that has one.

    Walks up from ``project_root``, skipping ``pyproject.toml`` files without a
    ``[tool.sqlty]`` table (like ruff), so a package's packaging-only
    ``pyproject.toml`` does not hide the workspace config. The closest table
    wins; tables are never merged.

    Args:
        project_root: Directory to start the upward search from; cwd if None.

    Returns:
        Tuple of (configuration dictionary, directory containing that
        pyproject.toml, or None if none was found or it failed to parse)
    """
    if project_root is None:
        project_root = Path.cwd()

    # Search for pyproject.toml up the directory tree
    current = project_root.resolve()
    while current != current.parent:
        pyproject_path = current / "pyproject.toml"
        if pyproject_path.exists():
            try:
                config = _parse_pyproject_cached(pyproject_path, pyproject_path.stat().st_mtime_ns)
            except Exception as e:
                logging.getLogger(__name__).warning("Failed to parse %s: %s", pyproject_path, e)
                return {}, None
            if config is not None:
                return copy.deepcopy(config), current
        current = current.parent

    # No [tool.sqlty] table found
    return {}, None


@lru_cache(maxsize=256)
def _parse_pyproject_cached(pyproject_path: Path, mtime_ns: int) -> SQLTyConfig | None:
    """Parse ``pyproject_path`` once per (path, mtime_ns); an edit changes the mtime and re-parses.

    The mtime is part of the key so long-running processes see edits. Failures
    are not cached.
    """
    return _parse_pyproject(pyproject_path)


def _parse_pyproject(pyproject_path: Path) -> SQLTyConfig | None:
    """Parse pyproject.toml and extract [tool.sqlty] section.

    Args:
        pyproject_path: Path to pyproject.toml

    Returns:
        Configuration dictionary, or None if there is no [tool.sqlty] table
    """
    import tomllib

    with open(pyproject_path, "rb") as f:
        data: dict[str, Any] = tomllib.load(f)

    # Extract [tool.sqlty] section
    tool_config: dict[str, Any] | None = data.get("tool", {}).get("sqlty")
    if tool_config is None:
        return None

    config: SQLTyConfig = {}

    # Parse each configuration option
    if "dialect" in tool_config:
        config["dialect"] = str(tool_config["dialect"])

    if "schemas" in tool_config and isinstance(tool_config["schemas"], dict):
        # Convert all values to strings
        config["schemas"] = {k: _schema_spec(v) for k, v in tool_config["schemas"].items()}

    # Single schema path (top-level convenience key)
    if "schema" in tool_config:
        config["schema"] = _schema_spec(tool_config["schema"])

    # Rule severity overrides: [tool.sqlty.rules]
    if "rules" in tool_config and isinstance(tool_config["rules"], dict):
        config["rules"] = {str(k): str(v) for k, v in tool_config["rules"].items()}

    # Allow hyphenated key in pyproject: format-command
    if "format-command" in tool_config:
        config["format_command"] = str(tool_config["format-command"])  # expects {filename}

    # Allow hyphenated key in pyproject: import-style
    if "import-style" in tool_config:
        config["import_style"] = str(tool_config["import-style"])

    if "untyped" in tool_config:
        config["untyped"] = str(tool_config["untyped"])

    # Parse function configurations
    if "functions" in tool_config and isinstance(tool_config["functions"], list):
        functions: list[FunctionConfigComplete] = []
        for func_config in tool_config["functions"]:
            if not isinstance(func_config, dict):
                continue

            # Validate required fields
            if "import_path" not in func_config or "query_param" not in func_config:
                logging.getLogger(__name__).warning(
                    "Skipping function config missing required fields: %s", func_config
                )
                continue

            # Build function config with all fields
            function_dict: dict[str, Any] = {
                "import_path": str(func_config["import_path"]),
                "query_param": str(func_config["query_param"]),
            }

            # Optional return_type_param field
            if "return_type_param" in func_config:
                function_dict["return_type_param"] = str(func_config["return_type_param"])

            # Optional schema field
            if "schema" in func_config:
                function_dict["schema"] = _schema_spec(func_config["schema"])

            # Optional return_type_template field
            if "return_type_template" in func_config:
                function_dict["return_type_template"] = str(func_config["return_type_template"])

            # Optional self_type_template field
            if "self_type_template" in func_config:
                function_dict["self_type_template"] = str(func_config["self_type_template"])

            # Optional imports field
            if "imports" in func_config:
                imports_value = func_config["imports"]
                if isinstance(imports_value, list):
                    function_dict["imports"] = [str(imp) for imp in imports_value]
                elif isinstance(imports_value, str):
                    # Support single string import
                    function_dict["imports"] = [str(imports_value)]

            # Cast to FunctionConfigComplete after all fields are set
            function = cast(FunctionConfigComplete, function_dict)
            functions.append(function)

        if functions:
            config["functions"] = functions

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

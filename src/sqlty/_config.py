"""Configuration loading from pyproject.toml."""

import copy
import difflib
import glob
import logging
from functools import lru_cache
from pathlib import Path
from typing import Any, NamedTuple, TypedDict, cast

SchemaSpec = str | list[str]

TOOL_KEYS = ("dialect", "schema", "rules", "import-style", "untyped", "type-modules", "functions")
"""Keys of ``[tool.sqlty]`` that are not file discovery; see also ``DISCOVERY_KEYS``."""

DISCOVERY_KEYS = ("exclude", "extend-exclude")
"""``[tool.sqlty]`` keys that pick the files to process."""

FUNCTION_KEYS = ("import-path", "query-param", "return-type-param")


class KeyDoc(NamedTuple):
    """Reference documentation of one configuration key.

    ``docs/reference/configuration.md`` renders its tables from these entries
    (``make generate-docs``); ``tests/test_docs.py`` fails when the page drifts.
    """

    type: str
    default: str
    used_by: str
    summary: str
    required: bool = False


KEY_DOCS: dict[str, KeyDoc] = {
    "dialect": KeyDoc(
        '`"postgres"`, `"sqlite"`, `"mysql"`',
        '`"postgres"`',
        "`rewrite`, `check`",
        "SQL dialect of queries and schema",
    ),
    "schema": KeyDoc(
        "string or list of strings",
        "none",
        "`rewrite`, `check`",
        "Schema `.sql` files, directories or globs, relative to `pyproject.toml`",
    ),
    "rules": KeyDoc(
        "table of rule = severity",
        "per-rule defaults",
        "`check`",
        'Severity per lint rule: `"error"`, `"warn"` or `"ignore"`',
    ),
    "import-style": KeyDoc(
        "string",
        '`"from sqlty import SQL"`',
        "`rewrite`, `strip`, `check`, magic codec",
        "How rewritten code imports and spells the wrapper",
    ),
    "untyped": KeyDoc(
        '`"skip"`, `"sentinel"`',
        '`"skip"`',
        "`rewrite`",
        "Whether queries of unknown row type are wrapped as `SQL[UNTYPED]`",
    ),
    "type-modules": KeyDoc(
        "list of strings",
        "`[]`",
        "`check`",
        "Modules `check` may import to resolve row type fields",
    ),
    "functions": KeyDoc(
        "array of tables",
        "`[]`",
        "`check`",
        "Helper functions whose string argument is a query",
    ),
    "exclude": KeyDoc(
        "list of globs",
        "see [Default excludes](#exclude-and-extend-exclude)",
        "`rewrite`, `strip`, `check`",
        "Globs skipped while walking a directory; replaces the default list",
    ),
    "extend-exclude": KeyDoc(
        "list of globs",
        "`[]`",
        "`rewrite`, `strip`, `check`",
        "Globs skipped in addition to `exclude` or the defaults",
    ),
}
"""Reference docs per key of ``[tool.sqlty]``, in documentation order."""

FUNCTION_KEY_DOCS: dict[str, KeyDoc] = {
    "import-path": KeyDoc(
        "string",
        "none",
        "`check`",
        'Module path and function name, e.g. `"app.db.sql"`; a method is `"pkg.db.Db.fetch_all"`. '
        'Use `"__main__.sql"` or the file name for a script run directly',
        required=True,
    ),
    "query-param": KeyDoc(
        "string",
        "none",
        "`check`",
        "Name of the parameter that holds the query",
        required=True,
    ),
    "return-type-param": KeyDoc(
        "string",
        "none",
        "`check`",
        "Parameter whose argument, such as `return_tp=int`, is the row type of that call "
        "instead of an inferred one; `check` compares it with the query columns",
    ),
}
"""Reference docs per key of a ``[[tool.sqlty.functions]]`` entry."""

DEFAULT_EXCLUDE = (
    ".bzr",
    ".direnv",
    ".eggs",
    ".git",
    ".git-rewrite",
    ".hg",
    ".ipynb_checkpoints",
    ".mypy_cache",
    ".nox",
    ".pants.d",
    ".pyenv",
    ".pytest_cache",
    ".pytype",
    ".ruff_cache",
    ".svn",
    ".tox",
    ".venv",
    ".vscode",
    "__pypackages__",
    "_build",
    "buck-out",
    "dist",
    "node_modules",
    "site-packages",
    "venv",
)
"""Directories skipped when walking a directory, unless ``exclude`` replaces them."""


class ConfigError(Exception):
    """A user error in configuration or command-line paths; the CLI prints it and exits 2."""


def _suggest(name: str, choices: tuple[str, ...] | list[str]) -> str:
    close = difflib.get_close_matches(name, choices, n=1)
    return f" (did you mean {close[0]!r}?)" if close else ""


def validate_tool_table(table: dict[str, Any], source: str) -> None:
    """Raise ``ConfigError`` for an unknown key, rule or severity in a ``[tool.sqlty]`` table.

    ``source`` names the file in the message. Shared by the CLI and the mdtest runner.

    Example:
        >>> validate_tool_table({"dialect": "mysql"}, "pyproject.toml")
        >>> validate_tool_table({"import_style": "x"}, "pyproject.toml")  # doctest: +ELLIPSIS
        Traceback (most recent call last):
        sqlty._config.ConfigError: pyproject.toml: unknown key 'import_style' in [tool.sqlty] (did you mean 'import-style'?); ...
    """
    from sqlty._lint import RULES, Severity

    keys = (*TOOL_KEYS, *DISCOVERY_KEYS)
    for key in table:
        if key not in keys:
            raise ConfigError(
                f"{source}: unknown key {key!r} in [tool.sqlty]{_suggest(key, keys)}; "
                f"expected one of: {', '.join(keys)}"
            )
    rules = table.get("rules", {})
    if not isinstance(rules, dict):
        raise ConfigError(f"{source}: [tool.sqlty] rules must be a table of rule = severity")
    severities = [s.value for s in Severity]
    for rule, severity in rules.items():
        if rule not in RULES:
            raise ConfigError(
                f"{source}: unknown rule {rule!r} in [tool.sqlty.rules]"
                f"{_suggest(rule, list(RULES))}; run `sqlty rule` for the list"
            )
        if severity not in severities:
            raise ConfigError(
                f"{source}: invalid severity {severity!r} for rule {rule!r} in "
                f"[tool.sqlty.rules]; expected one of: {', '.join(severities)}"
            )
    functions = table.get("functions", [])
    if not isinstance(functions, list):
        raise ConfigError(f"{source}: [tool.sqlty] functions must be an array of tables")
    for function in functions:
        if isinstance(function, dict):
            for key in function:
                if key not in FUNCTION_KEYS:
                    raise ConfigError(
                        f"{source}: unknown key {key!r} in [[tool.sqlty.functions]]"
                        f"{_suggest(key, FUNCTION_KEYS)}; expected one of: {', '.join(FUNCTION_KEYS)}"
                    )
    for key in DISCOVERY_KEYS:
        value = table.get(key, [])
        if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
            raise ConfigError(f"{source}: [tool.sqlty] {key} must be a list of glob strings")


class FunctionConfigComplete(TypedDict, total=False):
    """A ``[[tool.sqlty.functions]]`` entry: a helper whose string argument is a query.

    In TOML the keys are ``import-path`` and ``query-param`` (required) and
    ``return-type-param`` (optional); here they are the snake_case fields below.
    """

    import_path: str  # Full import path, e.g. "module.function_name" (required)
    query_param: str  # Parameter holding the SQL query (required)
    return_type_param: str  # Parameter holding the row type (optional)


class SQLTyConfig(TypedDict, total=False):
    """Configuration for SQLTy from pyproject.toml.

    Corresponds to the ``[tool.sqlty]`` section:

    .. code-block:: toml

        [tool.sqlty]
        dialect = "postgres"
        schema = "schema.sql"
        import-style = "from sqlty import SQL"
        untyped = "sentinel"
        type-modules = ["myapp.types"]
        extend-exclude = ["migrations"]

        [tool.sqlty.rules]
        select-star = "ignore"
        stale-type = "error"

        [[tool.sqlty.functions]]
        import-path = "db.query"
        query-param = "sql"
    """

    dialect: str
    schema: SchemaSpec  # Schema file, directory or glob, or a list of them
    rules: dict[str, str]  # rule-name -> severity (error/warn/ignore)
    import_style: str  # e.g., "from sqlty import SQL as Q" or "import sqlty"
    untyped: str  # "skip" (default) or "sentinel": SQL[UNTYPED] for uninferable queries
    type_modules: list[str]  # modules whose imports may run to resolve row types
    exclude: list[str]  # globs replacing the default excludes, relative to pyproject.toml
    extend_exclude: list[str]  # globs excluded in addition to the above
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
            except ConfigError:
                raise
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
    return _parse_table(tool_config, str(pyproject_path))


def load_config_file(path: Path) -> SQLTyConfig:
    """Load the configuration of a file named by ``--config``.

    A file named ``pyproject.toml`` must have a ``[tool.sqlty]`` table; any other
    file, such as ``sqlty.toml``, holds the same keys at the top level. Raises
    ``ConfigError`` when the file is missing, is not TOML, is a ``pyproject.toml``
    without ``[tool.sqlty]`` or has an invalid key.

    Example:
        >>> load_config_file(Path("missing.toml"))  # doctest: +ELLIPSIS
        Traceback (most recent call last):
        sqlty._config.ConfigError: config file does not exist: missing.toml
    """
    import tomllib

    if not path.exists():
        raise ConfigError(f"config file does not exist: {path}")
    if not path.is_file():
        raise ConfigError(f"config path is not a file: {path}")
    try:
        with open(path, "rb") as f:
            data: dict[str, Any] = tomllib.load(f)
    except tomllib.TOMLDecodeError as exc:
        raise ConfigError(f"{path}: {exc}") from None
    if path.name == "pyproject.toml":
        table = data.get("tool", {}).get("sqlty")
        if table is None:
            raise ConfigError(f"{path}: no [tool.sqlty] table")
        data = table
    return _parse_table(data, str(path))


def parse_overrides(assignments: list[str]) -> SQLTyConfig:
    """Parse ``-c/--config`` TOML assignments into one configuration; later ones win.

    Tables merge key by key, so ``rules.a = "ignore"`` and ``rules.b = "warn"``
    both apply. Raises ``ConfigError`` for a value that is not a TOML assignment
    or names an unknown key.

    Example:
        >>> parse_overrides(['dialect = "mysql"', 'rules.select-star = "ignore"'])
        {'dialect': 'mysql', 'rules': {'select-star': 'ignore'}}
    """
    import tomllib

    merged: dict[str, Any] = {}
    for assignment in assignments:
        if "=" not in assignment:
            raise ConfigError(
                f"--config expects a TOML assignment such as 'dialect = \"mysql\"', got "
                f"{assignment!r}; use --config-file for a file"
            )
        try:
            _merge(merged, tomllib.loads(assignment))
        except tomllib.TOMLDecodeError as exc:
            raise ConfigError(
                f"--config {assignment!r}: {exc}; quote strings, as in -c 'dialect = \"mysql\"', "
                "or use --config-file for a file"
            ) from None
    return _parse_table(merged, "--config")


def _merge(target: dict[str, Any], update: dict[str, Any]) -> None:
    for key, value in update.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            _merge(target[key], value)
        else:
            target[key] = value


def validate_rule_severity(pairs: list[tuple[str, str]]) -> dict[str, str]:
    """Rule severities from ``--error``/``--warn``/``--ignore RULE``, the last flag per rule winning.

    Example:
        >>> validate_rule_severity([("select-star", "error"), ("select-star", "ignore")])
        {'select-star': 'ignore'}
    """
    from sqlty._lint import RULES

    for rule, severity in pairs:
        if rule not in RULES:
            raise ConfigError(
                f"--{severity}: unknown rule {rule!r}{_suggest(rule, list(RULES))}; "
                "run `sqlty rule` for the list"
            )
    return dict(pairs)


def _parse_table(tool_config: dict[str, Any], source: str) -> SQLTyConfig:
    """Validate a ``[tool.sqlty]`` table and convert it to ``SQLTyConfig``."""
    validate_tool_table(tool_config, source)
    config: SQLTyConfig = {}

    if "exclude" in tool_config:
        config["exclude"] = list(tool_config["exclude"])
    if "extend-exclude" in tool_config:
        config["extend_exclude"] = list(tool_config["extend-exclude"])

    # Parse each configuration option
    if "dialect" in tool_config:
        config["dialect"] = str(tool_config["dialect"])

    # Single schema path (top-level convenience key)
    if "schema" in tool_config:
        config["schema"] = _schema_spec(tool_config["schema"])

    # Rule severity overrides: [tool.sqlty.rules]
    if "rules" in tool_config and isinstance(tool_config["rules"], dict):
        config["rules"] = {str(k): str(v) for k, v in tool_config["rules"].items()}

    # Allow hyphenated key in pyproject: import-style
    if "import-style" in tool_config:
        config["import_style"] = str(tool_config["import-style"])

    if "untyped" in tool_config:
        config["untyped"] = str(tool_config["untyped"])

    if "type-modules" in tool_config:
        modules = tool_config["type-modules"]
        config["type_modules"] = (
            [str(m) for m in modules] if isinstance(modules, list) else [str(modules)]
        )

    # Parse function configurations
    if "functions" in tool_config and isinstance(tool_config["functions"], list):
        functions: list[FunctionConfigComplete] = []
        for func_config in tool_config["functions"]:
            if not isinstance(func_config, dict):
                continue

            # Validate required fields
            if "import-path" not in func_config or "query-param" not in func_config:
                logging.getLogger(__name__).warning(
                    "Skipping function config missing required fields: %s", func_config
                )
                continue

            # Build function config with all fields
            function_dict: dict[str, Any] = {
                "import_path": str(func_config["import-path"]),
                "query_param": str(func_config["query-param"]),
            }

            # Optional return-type-param field
            if "return-type-param" in func_config:
                function_dict["return_type_param"] = str(func_config["return-type-param"])

            # Cast to FunctionConfigComplete after all fields are set
            function = cast(FunctionConfigComplete, function_dict)
            functions.append(function)

        if functions:
            config["functions"] = functions

    return config

"""Configuration loading from sqlty.toml, pyproject.toml or Cargo.toml."""

import logging
import re
from pathlib import Path
from typing import Any, NamedTuple

from sqlty import _core

Config = dict[str, Any]
"""A ``[tool.sqlty]`` table, its keys in snake_case."""

TOOL_KEYS = _core.TOOL_KEYS
"""Keys of ``[tool.sqlty]`` that are not file discovery; see also ``DISCOVERY_KEYS``."""

DISCOVERY_KEYS = _core.DISCOVERY_KEYS
"""``[tool.sqlty]`` keys that pick the files to process."""

FUNCTION_KEYS = _core.FUNCTION_KEYS

MAX_QUERY_VARIANTS = 64
"""Default of ``max-query-variants``."""


_LOWER_BOUND = re.compile(r"(?:>=|~=|==)\s*(\d+)\.(\d+)")


def requires_closed(requires_python: str | None) -> bool:
    """Whether ``requires-python`` rules out every Python before 3.15, which reads
    ``TypedDict(closed=True)`` (PEP 728).

    Example:
        >>> [requires_closed(r) for r in (">=3.15", "~=3.16", ">=3.14", ">3.14", None)]
        [True, True, False, False, False]
    """
    return any((int(a), int(b)) >= (3, 15) for a, b in _LOWER_BOUND.findall(requires_python or ""))


def driver_types(
    driver: str | None, dialect: str, types: dict[str, str] | None
) -> dict[str, str] | None:
    """``types`` with the defaults of ``driver`` under them, for a PostgreSQL project.

    Example:
        >>> driver_types("psycopg", "postgres", {"DATE": "str"})["INT4RANGE"]
        'psycopg.types.range.Range[int]'
        >>> driver_types("psycopg", "sqlite", None) is None
        True
    """
    if driver is None or dialect != "postgres":
        return types
    return dict(_core.driver_types(driver, dialect)) | (types or {})


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
        '`"postgres"`, `"sqlite"`, `"mysql"`, ...',
        '`"postgres"`',
        "`rewrite`, `check`",
        "SQL dialect of queries and schema",
    ),
    "driver": KeyDoc(
        '`"psycopg"`',
        "none",
        "`rewrite`, `check`",
        "Driver whose own classes type the columns the default mapping leaves untyped: PostgreSQL ranges",
    ),
    "schema": KeyDoc(
        "string or list of strings",
        "none",
        "`rewrite`, `check`",
        "Schema `.sql` files, directories or globs, relative to the configuration file",
    ),
    "queries": KeyDoc(
        "string or list of strings",
        "none",
        "`check`",
        "`.sql` files of named queries (aiosql, sqlc, PugSQL), directories or globs, relative to the configuration file",
    ),
    "query-format": KeyDoc(
        '`"auto"`, `"aiosql"`, `"sqlc"`, `"pugsql"`, `"file"`',
        '`"auto"`',
        "`check`",
        "How `queries` files name their queries; `auto` reads each file's first header",
    ),
    "rules": KeyDoc(
        "table of rule = severity",
        "per-rule defaults",
        "`check`",
        'Severity per lint rule: `"error"`, `"warn"` or `"ignore"`',
    ),
    "overrides": KeyDoc(
        "array of tables",
        "`[]`",
        "`check`",
        "Rule severities for the files `include` matches and `exclude` does not, on top of `rules`",
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
    "json-types": KeyDoc(
        '`"typing.Any"`, `"typing.TypedDict"`',
        '`"typing.Any"`',
        "`rewrite`, `check --fix`",
        "How a JSON column of known shape is typed: `typing.Any`, or a generated `TypedDict`",
    ),
    "param-types": KeyDoc(
        "`true`, `false`",
        "`true`",
        "`rewrite`, `check --fix`",
        "Whether `SQL[R]` calls get the type of their parameters, `SQL[R, P]`",
    ),
    "preview": KeyDoc(
        "boolean",
        "`false`",
        "`check`",
        "Turn on features still in preview: query variants",
    ),
    "max-query-variants": KeyDoc(
        "positive integer",
        "`64`",
        "`check`",
        "With `preview`, the most variants of a query built in branches that `check` checks",
    ),
    "coerce-row-types": KeyDoc(
        "list of strings",
        "`[]`",
        "`check`",
        "`TypedDict`, `NamedTuple` and dataclass rows, as `module.Name`, the code validates with Pydantic",
    ),
    "functions": KeyDoc(
        "array of tables",
        "`[]`",
        "`check`",
        "Helper functions whose string argument is a query",
    ),
    "stub-paths": KeyDoc(
        "list of strings",
        '`["typings"]`',
        "`check`",
        "Directories of `.pyi` stubs, read before the modules they describe to find the class of a helper method's receiver",
    ),
    "types": KeyDoc(
        "table of SQL type or `table.column` = Python type",
        "`{}`",
        "`rewrite`, `check`",
        "Python types a converter above the driver gives a SQL type or a column",
    ),
    "format-command": KeyDoc(
        "list of strings",
        "none",
        "`rewrite`, `check --fix`",
        "Formatter run on each file `rewrite` or `check --fix` changes, with the path appended",
    ),
    "extend": KeyDoc(
        "string",
        "none",
        "`rewrite`, `strip`, `check`, magic codec",
        "Another configuration file this one is applied on top of, relative to this file",
    ),
    "databases": KeyDoc(
        "table of tables",
        "`{}`",
        "`rewrite`, `check`",
        "Named databases, each with its own `dialect`, `schema` and the files it applies to",
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
    "respect-gitignore": KeyDoc(
        "boolean",
        "`true`",
        "`rewrite`, `strip`, `check`",
        "Skip the files `.gitignore` ignores while walking a directory",
    ),
}
"""Reference docs per key of ``[tool.sqlty]``, in documentation order."""

FUNCTION_KEY_DOCS: dict[str, KeyDoc] = {
    "import-path": KeyDoc(
        "string",
        "none",
        "`check`",
        'Module path and function name, for example `"app.db.sql"`; a method is `"pkg.db.Db.fetch_all"`. '
        "A package that re-exports it, `pkg.Db.fetch_all`, works too. "
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

DEFAULT_EXCLUDE: tuple[str, ...] = tuple(_core.DEFAULT_EXCLUDE)
"""Directories skipped when walking a directory, unless ``exclude`` replaces them."""


class ConfigError(Exception):
    """A user error in configuration or command-line paths; the CLI prints it and exits 2."""


def validate_tool_table(table: dict[str, Any], source: str) -> None:
    """Raise ``ConfigError`` for an unknown key, rule or severity in a ``[tool.sqlty]`` table.

    ``source`` names the file in the message. Shared by the CLI and the mdtest runner.

    Example:
        >>> validate_tool_table({"dialect": "mysql"}, "pyproject.toml")
        >>> validate_tool_table({"import_style": "x"}, "pyproject.toml")  # doctest: +ELLIPSIS
        Traceback (most recent call last):
        sqlty._config.ConfigError: pyproject.toml: unknown key 'import_style' in [tool.sqlty] (did you mean 'import-style'?); ...
    """
    _parse_table(table, source)


def load_config(project_root: Path | None = None) -> tuple[Config, Path | None]:
    """Load the nearest configuration above ``project_root``; see ``find_config``.

    Returns:
        Tuple of (configuration dictionary, directory containing its file, or None
        if none was found or a native file failed to parse)
    """
    config, path = find_config(project_root)
    return config, path.parent if path else None


def find_config(project_root: Path | None = None) -> tuple[Config, Path | None]:
    """Find the nearest ``sqlty.toml``, ``[tool.sqlty]`` of ``pyproject.toml`` or
    ``[package.metadata.sqlty]`` (else ``[workspace.metadata.sqlty]``) of ``Cargo.toml``,
    and the file it came from.

    Walks up from ``project_root`` (cwd if None). In each directory the files are
    tried in that order, and a native file without its sqlty table is skipped (like
    ruff), so a packaging-only ``pyproject.toml`` or ``Cargo.toml`` does not hide the
    workspace config. The closest file wins; files are never merged, except through
    ``extend``. A native file that fails to parse is logged and yields ``({}, None)``.
    """
    try:
        found = _core.find_config((project_root or Path.cwd()).resolve())
    except _core.ConfigSyntaxError as e:
        logging.getLogger(__name__).warning("Failed to parse %s", e)
        return {}, None
    except ValueError as exc:
        raise ConfigError(str(exc)) from None
    if found is None:
        return {}, None
    table, skipped, path = found
    return _converted(table, skipped, str(path)), path


def _parse_table(tool_config: dict[str, Any], source: str) -> Config:
    """Validate a ``[tool.sqlty]`` table and convert it to ``Config``."""
    try:
        found = _core.validate_config(tool_config, source)
    except ValueError as exc:
        raise ConfigError(str(exc)) from None
    return _converted(*found, source)


def _converted(table: dict[str, Any], skipped: list[str], source: str) -> Config:
    if "databases" in table:
        table["databases"] = {
            name: _converted(database, [], source) for name, database in table["databases"].items()
        }
    for entry in skipped:
        logging.getLogger(__name__).warning(
            "Skipping function config missing required fields: %s", entry
        )
    if "functions" in table:
        table["functions"] = [
            {key.replace("-", "_"): value for key, value in function.items()}
            for function in table["functions"]
        ]
    return {key.replace("-", "_"): value for key, value in table.items()}

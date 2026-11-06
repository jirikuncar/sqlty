"""Configuration loading from sqlty.toml, pyproject.toml or Cargo.toml."""

import ast
import copy
import difflib
import glob
import logging
import os
from functools import lru_cache
from pathlib import Path
from typing import Any, NamedTuple, TypedDict, cast

from sqlty import _core

SchemaSpec = str | list[str]

TOOL_KEYS = _core.TOOL_KEYS
"""Keys of ``[tool.sqlty]`` that are not file discovery; see also ``DISCOVERY_KEYS``."""

DISCOVERY_KEYS = _core.DISCOVERY_KEYS
"""``[tool.sqlty]`` keys that pick the files to process."""

FUNCTION_KEYS = _core.FUNCTION_KEYS

MAX_QUERY_VARIANTS = 64
"""Default of ``max-query-variants``."""


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
        "`false`",
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
    "functions": KeyDoc(
        "array of tables",
        "`[]`",
        "`check`",
        "Helper functions whose string argument is a query",
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


def dialect_problem(dialect: object) -> str | None:
    """Why ``dialect`` is not a dialect name, or ``None`` when it is one (in any case).

    Example:
        >>> dialect_problem("PostgreSQL") is None
        True
        >>> dialect_problem("postgress")  # doctest: +ELLIPSIS
        "unknown dialect 'postgress' (did you mean 'postgres'?); expected one of: postgres, ..."
    """
    from sqlty._core import dialect_names

    names = dialect_names()
    if isinstance(dialect, str) and dialect.lower() in (*names, "postgresql", "tsql"):
        return None
    return (
        f"unknown dialect {dialect!r}{_suggest(str(dialect), names)}; "
        f"expected one of: {', '.join(names)}"
    )


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


class RuleOverride(TypedDict):
    """A ``[[tool.sqlty.overrides]]`` entry, as ty spells them: rule severities for the files
    ``include`` matches (every file when it is empty) and ``exclude`` does not.

    Globs are relative to the ``pyproject.toml`` directory; a glob without a slash
    matches a file or directory name at any depth, as in ``exclude``.
    """

    include: list[str]
    exclude: list[str]
    rules: dict[str, str]


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
    queries: SchemaSpec  # .sql files of named queries, as files, directories or globs
    query_format: str  # "auto" (default), "aiosql", "sqlc", "pugsql" or "file"
    rules: dict[str, str]  # rule-name -> severity (error/warn/ignore)
    overrides: list[RuleOverride]  # rule severities for files matching globs
    import_style: str  # e.g., "from sqlty import SQL as Q" or "import sqlty"
    untyped: str  # "skip" (default) or "sentinel": SQL[UNTYPED] for uninferable queries
    json_types: (
        str  # "typing.Any" (default) or "typing.TypedDict": generated TypedDicts for JSON columns
    )
    param_types: bool  # write the P of SQL[R, P]: a tuple, or a generated TypedDict
    preview: bool  # features still in preview: query variants
    max_query_variants: int  # the most variants of a query built in branches that check checks
    exclude: list[str]  # globs replacing the default excludes, relative to the configuration file
    extend_exclude: list[str]  # globs excluded in addition to the above
    respect_gitignore: bool  # skip what .gitignore ignores while walking
    functions: list[FunctionConfigComplete]  # List of functions to track and analyze
    types: dict[str, str]  # SQL type or table.column -> Python type a converter gives it
    format_command: list[str]  # formatter run on changed files, path appended
    extend: str  # another configuration file this one is applied on top of


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
    """Load the nearest configuration above ``project_root``; see ``find_config``.

    Returns:
        Tuple of (configuration dictionary, directory containing its file, or None
        if none was found or a native file failed to parse)
    """
    config, path = find_config(project_root)
    return config, path.parent if path else None


def find_config(project_root: Path | None = None) -> tuple[SQLTyConfig, Path | None]:
    """Find the nearest ``sqlty.toml``, ``[tool.sqlty]`` of ``pyproject.toml`` or
    ``[package.metadata.sqlty]`` (else ``[workspace.metadata.sqlty]``) of ``Cargo.toml``,
    and the file it came from.

    Walks up from ``project_root`` (cwd if None). In each directory the files are
    tried in that order, and a native file without its sqlty table is skipped (like
    ruff), so a packaging-only ``pyproject.toml`` or ``Cargo.toml`` does not hide the
    workspace config. The closest file wins; files are never merged. A native file
    that fails to parse is logged and yields ``({}, None)``.
    """
    current = (project_root or Path.cwd()).resolve()
    while current != current.parent:
        sqlty_toml = current / "sqlty.toml"
        if sqlty_toml.is_file():
            config = _load_config_file_cached(sqlty_toml, sqlty_toml.stat().st_mtime_ns)
            return _extended(copy.deepcopy(config), sqlty_toml, current), sqlty_toml
        for name in _core.CONFIG_FILES[1:]:
            native = current / name
            if not native.exists():
                continue
            try:
                found = _parse_native_cached(native, native.stat().st_mtime_ns)
            except ConfigError:
                raise
            except (_core.ConfigSyntaxError, OSError) as e:
                logging.getLogger(__name__).warning("Failed to parse %s", e)
                return {}, None
            if found is not None:
                return _extended(copy.deepcopy(found), native, current), native
        current = current.parent
    return {}, None


@lru_cache(maxsize=256)
def _parse_native_cached(path: Path, mtime_ns: int) -> SQLTyConfig | None:
    """Parse ``path`` once per (path, mtime_ns); an edit changes the mtime and re-parses.

    The mtime is part of the key so long-running processes see edits. Failures
    are not cached.
    """
    return _parse_native(path)


@lru_cache(maxsize=256)
def _load_config_file_cached(path: Path, mtime_ns: int) -> SQLTyConfig:
    return _read_config_file(path)


def _parse_native(path: Path) -> SQLTyConfig | None:
    """The sqlty table of a ``pyproject.toml`` or ``Cargo.toml``, or None without one."""
    return _load(path, required=False)


def load_config_file(path: Path) -> SQLTyConfig:
    """Load the configuration of a file named by ``--config``.

    A file named ``pyproject.toml`` must have a ``[tool.sqlty]`` table and one named
    ``Cargo.toml`` a ``[package.metadata.sqlty]`` or ``[workspace.metadata.sqlty]``
    table; any other file, such as ``sqlty.toml``, holds the same keys at the top level. Raises ``ConfigError`` when
    the file is missing, is not TOML, lacks its table or has an invalid key.

    Example:
        >>> load_config_file(Path("missing.toml"))  # doctest: +ELLIPSIS
        Traceback (most recent call last):
        sqlty._config.ConfigError: config file does not exist: missing.toml
    """
    return _extended(_read_config_file(path), path, Path.cwd().resolve())


def _extended(
    config: SQLTyConfig, path: Path, base: Path, chain: tuple[Path, ...] = ()
) -> SQLTyConfig:
    """``config`` of ``path`` on top of the file its ``extend`` names, recursively.

    ``rules`` and ``types`` merge key by key; any other key of ``config`` replaces the
    extended one. ``schema`` and ``queries`` of an extended file are rebased onto ``base``,
    the directory the relative paths of ``config`` start from.
    """
    extend = config.pop("extend", None)
    if extend is None:
        return config
    chain = (*chain, path.resolve())
    target = (path.parent / extend).resolve()
    if target in chain:
        raise ConfigError(f"{path}: extend = {extend!r} extends itself in a cycle")
    if not target.is_file():
        raise ConfigError(f"{path}: extend = {extend!r}: config file does not exist: {target}")
    parent = _extended(
        copy.deepcopy(_load_config_file_cached(target, target.stat().st_mtime_ns)),
        target,
        target.parent,
        chain,
    )
    for key in ("schema", "queries"):
        if key in parent:
            spec = parent[key]
            parent[key] = [
                os.path.relpath(target.parent / entry, base)
                for entry in ([spec] if isinstance(spec, str) else spec)
            ]
    merged = cast(SQLTyConfig, {**parent, **config})
    for key in ("rules", "types"):
        if key in parent and key in config:
            merged[key] = {**parent[key], **config[key]}  # type: ignore[literal-required]
    return merged


def _read_config_file(path: Path) -> SQLTyConfig:
    if not path.exists():
        raise ConfigError(f"config file does not exist: {path}")
    if not path.is_file():
        raise ConfigError(f"config path is not a file: {path}")
    try:
        config = _load(path, required=True)
    except _core.ConfigSyntaxError as exc:
        raise ConfigError(str(exc)) from None
    assert config is not None
    return config


def _load(path: Path, *, required: bool) -> SQLTyConfig | None:
    data = path.read_bytes()
    try:
        found = _core.load_config(data, path.name, str(path), required=required)
    except _core.ConfigSyntaxError:
        raise
    except ValueError as exc:
        raise ConfigError(str(exc)) from None
    return None if found is None else _converted(*found, str(path))


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
    try:
        found = _core.validate_config(tool_config, source)
    except ValueError as exc:
        raise ConfigError(str(exc)) from None
    return _converted(*found, source)


def _converted(table: dict[str, Any], skipped: list[str], source: str) -> SQLTyConfig:
    for key, python in table.get("types", {}).items():
        try:
            ast.parse(python, mode="eval")
        except SyntaxError:
            raise ConfigError(
                f"{source}: [tool.sqlty.types] {key} = {python!r} is not a Python type expression"
            ) from None
    for entry in skipped:
        logging.getLogger(__name__).warning(
            "Skipping function config missing required fields: %s", entry
        )
    if "functions" in table:
        table["functions"] = [
            {key.replace("-", "_"): value for key, value in function.items()}
            for function in table["functions"]
        ]
    return cast(SQLTyConfig, {key.replace("-", "_"): value for key, value in table.items()})

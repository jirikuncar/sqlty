"""CLI entry point — ruff-style subcommand router.

Commands:
    sqlty rewrite [OPTIONS] [PATH...]   Rewrite source files
    sqlty strip [OPTIONS] [PATH...]     Strip SQL[T](...) wrappers
    sqlty check [OPTIONS] [PATH...]     Lint SQL type annotations
    sqlty rule [NAME]                   Explain a lint rule, or list them
    sqlty version                       Print version
"""

from __future__ import annotations

import fnmatch
import functools
import gc
import logging
import os
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import argparse
    from collections import Counter
    from collections.abc import Callable

    from sqlty._config import SchemaSpec

logger = logging.getLogger(__name__)

EXIT_FOUND = 1
"""``check`` reported an error, or ``--check``/``--diff`` found files to change."""
EXIT_ERROR = 2
"""Bad usage or configuration, a missing path, or an internal error."""


def main(argv: list[str] | None = None) -> None:
    """CLI entry point."""
    parser = build_parser()
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)

    if args.version:
        _cmd_version()
        return

    if args.command is None:
        parser.print_help()
        sys.exit(0)

    from sqlty._config import ConfigError

    try:
        match args.command:
            case "rewrite":
                _cmd_rewrite(args)
            case "strip":
                _cmd_strip(args)
            case "check":
                _cmd_check(args)
            case "rule":
                _cmd_rule(args.name)
            case "version":
                _cmd_version()
            case _:
                parser.print_help()
    except ConfigError as exc:
        print(f"sqlty: error: {exc}", file=sys.stderr)
        sys.exit(EXIT_ERROR)
    except Exception:
        import traceback

        traceback.print_exc()
        print("sqlty: internal error; please report it with the traceback above", file=sys.stderr)
        sys.exit(EXIT_ERROR)


def build_parser() -> argparse.ArgumentParser:
    """The ``sqlty`` argument parser; ``docs/reference/cli.md`` is rendered from it."""
    import argparse

    parser = argparse.ArgumentParser(
        prog="sqlty",
        description="Type-safe SQL queries for Python.",
    )
    parser.add_argument("--version", action="store_true", help="Print version and exit")

    subparsers = parser.add_subparsers(dest="command")

    # --- rewrite ---
    rewrite_parser = subparsers.add_parser(
        "rewrite",
        help='Rewrite "--sql\\n..." strings into SQL[T]("--sql\\n...") calls',
    )
    _add_common_args(rewrite_parser)
    rewrite_parser.add_argument(
        "--check",
        action="store_true",
        help="Exit 1 if files need rewriting (don't modify files)",
    )
    rewrite_parser.add_argument(
        "--diff",
        action="store_true",
        help="Show diff without applying changes",
    )
    rewrite_parser.add_argument(
        "--untyped",
        choices=["skip", "sentinel"],
        help="Row-returning queries of unknown row type: leave unwrapped (skip, default) "
        "or wrap as SQL[UNTYPED](...) (sentinel). Config: untyped in [tool.sqlty]",
    )

    # --- strip ---
    strip_parser = subparsers.add_parser(
        "strip",
        help='Strip SQL[T]("--sql\\n...") wrappers back to plain strings',
    )
    _add_common_args(strip_parser)
    strip_parser.add_argument(
        "--check",
        action="store_true",
        help="Exit 1 if files contain SQL[T](...) wrappers",
    )
    strip_parser.add_argument(
        "--diff",
        action="store_true",
        help="Show diff without applying changes",
    )

    # --- check ---
    check_parser = subparsers.add_parser(
        "check",
        help="Lint: validate SQL type annotations and detect issues",
    )
    _add_common_args(check_parser)
    check_parser.add_argument(
        "--fix",
        action="store_true",
        help="Fix what can be fixed (the rules `sqlty rule` marks fixable, e.g. stale-rewrite, "
        "stale-type, select-star with a schema), then report the rest",
    )
    check_parser.add_argument(
        "--statistics",
        action="store_true",
        help="After the diagnostics, count queries typed, partially typed, unparsed and skipped",
    )

    # --- rule ---
    rule_parser = subparsers.add_parser(
        "rule",
        help="Explain a lint rule (sqlty rule stale-type), or list all rules",
    )
    rule_parser.add_argument("name", nargs="?", help="Rule name, e.g. select-star")

    # --- version ---
    subparsers.add_parser("version", help="Print version and exit")
    return parser


def _add_common_args(parser: argparse.ArgumentParser) -> None:
    """Add arguments shared across rewrite/strip/check."""
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        default=[Path(".")],
        help="Files or directories (default: .); each file uses its nearest pyproject.toml",
    )
    parser.add_argument(
        "--dialect",
        choices=["postgres", "sqlite", "mysql"],
        help="SQL dialect (default: [tool.sqlty] dialect, else postgres)",
    )
    parser.add_argument(
        "--schema",
        action="append",
        help="Schema .sql file, directory or glob for every path; repeat for several "
        "(default: [tool.sqlty] schema per project)",
    )
    parser.add_argument("-v", "--verbose", action="count", default=0, help="Increase verbosity")
    parser.add_argument("-q", "--quiet", action="store_true", help="Suppress non-error output")


# --- Command implementations ---

_SKIPPED_STRING_RULES = ("fstring-skipped", "tstring-skipped", "unchecked-query")


def _cmd_rewrite(args: argparse.Namespace) -> None:
    """Rewrite command: transform --sql strings into SQL[T](...) calls.

    f- and t-strings starting with ``--sql`` are never rewritten; they are
    reported on stderr (``fstring-skipped``, ``tstring-skipped``) unless ``-q``.
    """
    from sqlty._lint import RULES
    from sqlty.core import check_source, rewrite_source

    def transform(path: Path, original: str, settings: Settings) -> str:
        rewritten = rewrite_source(
            original,
            dialect=settings.dialect,
            schema_sql=settings.schema_sql,
            import_style=settings.import_style,
            untyped=settings.untyped,
        )
        if not args.quiet:
            skipped_only = dict.fromkeys(RULES, "ignore") | {
                rule: (settings.rules or {}).get(rule, RULES[rule].default_severity.value)
                for rule in _SKIPPED_STRING_RULES
            }
            for diag in check_source(
                original, import_style=settings.import_style, rules=skipped_only
            ):
                print(f"{path}:{diag['line']}: [{diag['rule']}] {diag['message']}", file=sys.stderr)
        return rewritten

    _transform_files(args, transform, "rewrite", "Rewrote", "rewritten")


def _cmd_strip(args: argparse.Namespace) -> None:
    """Strip command: remove SQL[T](...) wrappers."""
    from sqlty.core import strip_source

    def transform(path: Path, original: str, settings: Settings) -> str:
        return strip_source(original, import_style=settings.import_style)

    _transform_files(args, transform, "strip", "Stripped", "stripped")


def _transform_files(
    args: argparse.Namespace,
    transform: Callable[[Path, str, Settings], str],
    verb: str,
    done: str,
    participle: str,
) -> None:
    """Apply ``transform`` to every collected file for ``rewrite`` and ``strip``.

    Writes the result back. With ``--check`` or ``--diff`` it leaves the files
    alone and exits 1 when any would change.
    """
    resolver = SettingsResolver(args)
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path in files:
        original = path.read_text(encoding="utf-8")
        result = transform(path, original, resolver.for_file(path))
        if result == original:
            continue

        changed += 1
        if args.check:
            if not args.quiet:
                print(f"Would {verb} {path}")
        elif args.diff:
            _print_diff(path, original, result)
        else:
            path.write_text(result, encoding="utf-8")
            if args.verbose:
                print(f"{done} {path}")

    if args.check or args.diff:
        if changed > 0:
            if args.check and not args.quiet:
                print(f"{changed} file(s) would be {participle}")
            sys.exit(EXIT_FOUND)
        elif args.check and not args.quiet:
            print("All files up to date")


def _cmd_check(args: argparse.Namespace) -> None:
    """Check command: lint SQL type annotations."""
    from collections import Counter

    from sqlty._source import ParsedSource
    from sqlty.core import HelperIndex, check_source, fix_source, query_statistics

    # Parsed trees stay alive for cross-module lookups: collect young objects only, as mypy does.
    gc.set_threshold(200_000, 30, 30)
    resolver = SettingsResolver(args)
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    modules = {path: _module_names(path, args.paths) for path in files}
    sources = {
        path: ParsedSource(
            path.read_text(encoding="utf-8"),
            modules[path][0] + (".__init__" if path.name == "__init__.py" else ""),
        )
        for path in files
    }
    helpers = HelperIndex()
    for path, parsed in sources.items():
        for name in modules[path]:
            helpers.add(name, parsed)
    configured: dict[tuple[tuple[str, str, str | None], ...], HelperIndex] = {}

    def helpers_for(settings: Settings) -> HelperIndex:
        """The index for files governed by ``settings``: its own ``functions`` only."""
        if not settings.functions:
            return helpers
        if settings.functions not in configured:
            configured[settings.functions] = index = helpers.fork()
            for import_path, query_param, return_param in settings.functions:
                index.register(import_path, query_param, return_param)
        return configured[settings.functions]

    total_errors = 0
    total_warnings = 0
    total_fixed = 0
    statistics: Counter[str] = Counter()

    for path in files:
        parsed = sources[path]
        settings = resolver.for_file(path)
        file_helpers = helpers_for(settings)
        if args.fix:
            fixed = fix_source(
                parsed,
                dialect=settings.dialect,
                schema_sql=settings.schema_sql,
                import_style=settings.import_style,
                untyped=settings.untyped,
                rules=settings.rules,
            )
            if fixed != parsed.text:
                path.write_text(fixed, encoding="utf-8")
                parsed = ParsedSource(fixed, parsed.module)
                total_fixed += 1
                if args.verbose:
                    print(f"Fixed {path}")
        diagnostics = check_source(
            parsed,
            dialect=settings.dialect,
            schema_sql=settings.schema_sql,
            rules=settings.rules,
            import_style=settings.import_style,
            untyped=settings.untyped,
            helpers=file_helpers,
            type_modules=settings.type_modules,
        )
        if args.statistics:
            statistics += query_statistics(
                parsed,
                dialect=settings.dialect,
                schema_sql=settings.schema_sql,
                import_style=settings.import_style,
                helpers=file_helpers,
            )

        for diag in diagnostics:
            severity = diag["severity"]
            line = diag.get("line", "")
            rule = diag["rule"]
            message = diag["message"]

            if severity == "error":
                total_errors += 1
            elif severity == "warn":
                total_warnings += 1

            if not args.quiet:
                print(f"{path}:{line}: [{rule}] {message}")

    if total_fixed and not args.quiet:
        print(f"Fixed {total_fixed} file(s)")
    if not args.quiet and (total_errors or total_warnings):
        parts = []
        if total_errors:
            parts.append(f"{total_errors} error(s)")
        if total_warnings:
            parts.append(f"{total_warnings} warning(s)")
        print(f"Found {', '.join(parts)}")
    if args.statistics and not args.quiet:
        print(_format_statistics(statistics))

    if total_errors > 0:
        sys.exit(EXIT_FOUND)


def _format_statistics(counts: Counter[str]) -> str:
    """One line of query counts; skipped strings are broken down by rule."""
    skipped = {rule: counts[rule] for rule in _SKIPPED_STRING_RULES if counts[rule]}
    detail = ", ".join(f"{rule}: {count}" for rule, count in skipped.items())
    return (
        f"Queries: {counts['typed']} typed, {counts['partially typed']} partially typed, "
        f"{counts['unparsed']} unparsed, {sum(skipped.values())} skipped"
        + (f" ({detail})" if detail else "")
    )


def _cmd_rule(name: str | None) -> None:
    """Rule command: print a rule's documentation, or the rule table."""
    from sqlty._lint import RULES, rule_doc

    if name is None:
        width = max(map(len, RULES))
        for rule in RULES.values():
            print(f"{rule.name:<{width}}  {rule.default_severity.value:<5}  {rule.description}")
        return
    if name not in RULES:
        logger.error("Unknown rule %r; run `sqlty rule` for the list", name)
        sys.exit(EXIT_ERROR)
    print(rule_doc(name), end="")


def _cmd_version() -> None:
    """Print version."""
    from sqlty import __version__

    print(f"sqlty {__version__}")


# --- File helpers ---


def _collect_python_files(paths: list[Path]) -> list[Path]:
    """Collect .py files from paths (files and directories).

    Files named on the command line are always collected. Walking a directory
    skips the ``exclude`` / ``extend-exclude`` globs of the nearest config, or
    the default excludes (``.git``, ``.venv``, ``node_modules``, ...).
    A path that does not exist raises ``ConfigError``.
    """
    from sqlty._config import ConfigError

    files: list[Path] = []
    for path in paths:
        if path.is_file():
            if path.suffix == ".py":
                files.append(path)
        elif path.is_dir():
            files.extend(_walk(path))
        else:
            raise ConfigError(f"path does not exist: {path}")
    return files


def _exclusions(directory: Path) -> tuple[Path, tuple[str, ...]]:
    """Base directory and exclude globs governing ``directory``."""
    from sqlty._config import DEFAULT_EXCLUDE, load_config

    config, config_dir = load_config(directory)
    patterns = (*config.get("exclude", DEFAULT_EXCLUDE), *config.get("extend_exclude", ()))
    return (config_dir or directory.resolve()), patterns


def _is_excluded(path: Path, base: Path, patterns: tuple[str, ...]) -> bool:
    """Whether ``path`` matches a glob, relative to ``base``.

    A glob without a slash matches a file or directory name at any depth;
    otherwise it matches the path relative to ``base``.
    """
    resolved = path.resolve()
    if not resolved.is_relative_to(base):
        return False
    relative = PurePosixPath(resolved.relative_to(base).as_posix())
    for pattern in patterns:
        pattern = pattern.strip("/")
        if "/" not in pattern and fnmatch.fnmatchcase(path.name, pattern):
            return True
        if relative.full_match(pattern):
            return True
    return False


def _walk(root: Path) -> list[Path]:
    """The ``.py`` files under ``root``, sorted, without excluded directories and files."""
    found: list[Path] = []
    for directory, dirnames, filenames in os.walk(root):
        current = Path(directory)
        base, patterns = _exclusions(current)
        dirnames[:] = sorted(d for d in dirnames if not _is_excluded(current / d, base, patterns))
        found.extend(
            current / name
            for name in sorted(filenames)
            if name.endswith(".py") and not _is_excluded(current / name, base, patterns)
        )
    return sorted(found)


@functools.cache
def _is_package(directory: Path) -> bool:
    return (directory / "__init__.py").exists()


@functools.cache
def _package_path(directory: Path) -> tuple[str, ...]:
    """Names of ``directory`` and its parents up through the ``__init__.py`` ones."""
    if directory == directory.parent or not _is_package(directory):
        return ()
    return (*_package_path(directory.parent), directory.name)


@functools.cache
def _resolved_root(root: Path) -> Path | None:
    """``root`` resolved, or ``None`` when it is not a directory."""
    root = root.resolve()
    return root if root.is_dir() else None


def _module_names(path: Path, roots: list[Path]) -> list[str]:
    """Dotted names ``path`` can be imported by, the likeliest first.

    The package path (up through the ``__init__.py`` directories) comes first,
    then the path from each checked directory, with and without its own name.
    ``pkg/__init__.py`` is ``pkg``.

    Example:
        >>> _module_names(Path("missing/pkg/mod.py"), [])
        ['mod']
    """
    path = path.resolve()
    parts = [*_package_path(path.parent), *([] if path.stem == "__init__" else [path.stem])]
    names = [".".join(parts)]
    for root in map(_resolved_root, roots):
        if root is not None and path.is_relative_to(root):
            relative = list(path.relative_to(root).with_suffix("").parts)
            if relative[-1] == "__init__":
                relative.pop()
            names += [".".join(relative), ".".join([root.name, *relative])]
    return list(dict.fromkeys(name for name in names if name)) or [path.stem]


@dataclass(frozen=True)
class Settings:
    """``[tool.sqlty]`` of the nearest ``pyproject.toml``, with CLI flags on top."""

    dialect: str = "postgres"
    schema_sql: str | None = None
    import_style: str | None = None
    untyped: bool = False
    rules: dict[str, str] | None = None
    type_modules: tuple[str, ...] = ()
    functions: tuple[tuple[str, str, str | None], ...] = ()


class SettingsResolver:
    """Resolve settings per file, so each project directory uses its own config.

    Example:
        >>> import argparse
        >>> args = argparse.Namespace(dialect="sqlite", schema=None, untyped=None)
        >>> SettingsResolver(args).for_file(Path("missing/dir/q.py")).dialect
        'sqlite'
    """

    def __init__(self, args: argparse.Namespace) -> None:
        self._args = args
        self._schema_sql = _load_schema(args.schema, Path.cwd()) if args.schema else None
        self._by_dir: dict[Path, Settings] = {}
        self._warned: set[tuple[str, str]] = set()

    def _warn_skipped_objects(self, schema_sql: str | None, dialect: str) -> None:
        from sqlty.core import schema_skipped_objects

        if schema_sql is None or (schema_sql, dialect) in self._warned:
            return
        self._warned.add((schema_sql, dialect))
        for kind, name in schema_skipped_objects(schema_sql, dialect=dialect):
            logger.warning("Schema %s %s could not be parsed; its columns are unknown", kind, name)

    def for_file(self, path: Path) -> Settings:
        directory = path.parent.resolve()
        if directory not in self._by_dir:
            self._by_dir[directory] = self._resolve(directory)
        return self._by_dir[directory]

    def _resolve(self, directory: Path) -> Settings:
        from sqlty._config import ConfigError, load_config
        from sqlty.core import strip_source

        config, config_dir = load_config(directory)
        args = self._args
        schema_sql = self._schema_sql
        if schema_sql is None and "schema" in config and config_dir is not None:
            schema_sql = _load_schema(config["schema"], config_dir)
        untyped = getattr(args, "untyped", None) or config.get("untyped", "skip")
        if untyped not in ("skip", "sentinel"):
            raise ConfigError(f"{config_dir}/pyproject.toml: untyped must be 'skip' or 'sentinel'")
        import_style = config.get("import_style")
        try:
            strip_source("", import_style=import_style)
        except ValueError as exc:
            raise ConfigError(
                f"{config_dir}/pyproject.toml: {exc}; use 'from sqlty import SQL [as Q]' "
                "or 'import sqlty [as sy]'"
            ) from None
        dialect = args.dialect or config.get("dialect", "postgres")
        self._warn_skipped_objects(schema_sql, dialect)
        return Settings(
            dialect=dialect,
            schema_sql=schema_sql,
            import_style=import_style,
            untyped=untyped == "sentinel",
            rules=config.get("rules"),
            type_modules=tuple(config.get("type_modules", ())),
            functions=tuple(
                (f["import_path"], f["query_param"], f.get("return_type_param"))
                for f in config.get("functions", [])
            ),
        )


def _load_schema(spec: SchemaSpec, base: Path) -> str | None:
    """Concatenate the schema files named by ``spec``; None if none can be read."""
    from sqlty._config import expand_schema

    parts: list[str] = []
    for path in expand_schema(spec, base):
        if path.is_file():
            parts.append(path.read_text(encoding="utf-8"))
        else:
            logger.warning("Schema file not found: %s", path)
    return "\n".join(parts) if parts else None


def _print_diff(path: Path, original: str, modified: str) -> None:
    """Print unified diff between original and modified."""
    import difflib

    diff = difflib.unified_diff(
        original.splitlines(keepends=True),
        modified.splitlines(keepends=True),
        fromfile=str(path),
        tofile=str(path),
    )
    sys.stdout.writelines(diff)

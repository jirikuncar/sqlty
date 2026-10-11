"""CLI entry point — ruff-style subcommand router.

Commands:
    sqlty rewrite [OPTIONS] [PATH...]   Rewrite source files
    sqlty strip [OPTIONS] [PATH...]     Strip SQL[T](...) wrappers
    sqlty check [OPTIONS] [PATH...]     Lint SQL type annotations
    sqlty rule [NAME]                   Explain a lint rule, or list them
    sqlty version                       Print version
"""

from __future__ import annotations

import logging
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import argparse

logger = logging.getLogger(__name__)


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
        type=Path,
        help="Schema .sql file for every path (default: [tool.sqlty] schema per project)",
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

    resolver = SettingsResolver(args)
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path in files:
        original = path.read_text(encoding="utf-8")
        settings = resolver.for_file(path)
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

        if rewritten == original:
            continue

        changed += 1
        if args.check:
            if not args.quiet:
                print(f"Would rewrite {path}")
        elif args.diff:
            _print_diff(path, original, rewritten)
        else:
            path.write_text(rewritten, encoding="utf-8")
            if args.verbose:
                print(f"Rewrote {path}")

    if args.check:
        if changed > 0:
            if not args.quiet:
                print(f"{changed} file(s) would be rewritten")
            sys.exit(1)
        elif not args.quiet:
            print("All files up to date")


def _cmd_strip(args: argparse.Namespace) -> None:
    """Strip command: remove SQL[T](...) wrappers."""
    from sqlty.core import strip_source

    resolver = SettingsResolver(args)
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path in files:
        original = path.read_text(encoding="utf-8")
        stripped = strip_source(original, import_style=resolver.for_file(path).import_style)

        if stripped == original:
            continue

        changed += 1
        if args.check:
            if not args.quiet:
                print(f"Would strip {path}")
        elif args.diff:
            _print_diff(path, original, stripped)
        else:
            path.write_text(stripped, encoding="utf-8")
            if args.verbose:
                print(f"Stripped {path}")

    if args.check:
        if changed > 0:
            if not args.quiet:
                print(f"{changed} file(s) would be stripped")
            sys.exit(1)
        elif not args.quiet:
            print("All files up to date")


def _cmd_check(args: argparse.Namespace) -> None:
    """Check command: lint SQL type annotations."""
    from sqlty.core import check_source, fix_source

    resolver = SettingsResolver(args)
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    total_errors = 0
    total_warnings = 0
    total_fixed = 0

    for path in files:
        source = path.read_text(encoding="utf-8")
        settings = resolver.for_file(path)
        if args.fix:
            fixed = fix_source(
                source,
                dialect=settings.dialect,
                schema_sql=settings.schema_sql,
                import_style=settings.import_style,
                untyped=settings.untyped,
                rules=settings.rules,
            )
            if fixed != source:
                path.write_text(fixed, encoding="utf-8")
                source = fixed
                total_fixed += 1
                if args.verbose:
                    print(f"Fixed {path}")
        diagnostics = check_source(
            source,
            dialect=settings.dialect,
            schema_sql=settings.schema_sql,
            rules=settings.rules,
            import_style=settings.import_style,
            untyped=settings.untyped,
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

    if total_errors > 0:
        sys.exit(1)


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
        sys.exit(2)
    print(rule_doc(name), end="")


def _cmd_version() -> None:
    """Print version."""
    from sqlty import __version__

    print(f"sqlty {__version__}")


# --- File helpers ---


def _collect_python_files(paths: list[Path]) -> list[Path]:
    """Collect .py files from paths (files and directories)."""
    files: list[Path] = []
    for path in paths:
        if path.is_file() and path.suffix == ".py":
            files.append(path)
        elif path.is_dir():
            files.extend(sorted(path.rglob("*.py")))
    return files


@dataclass(frozen=True)
class Settings:
    """``[tool.sqlty]`` of the nearest ``pyproject.toml``, with CLI flags on top."""

    dialect: str = "postgres"
    schema_sql: str | None = None
    import_style: str | None = None
    untyped: bool = False
    rules: dict[str, str] | None = None


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
        self._schema_sql = _load_schema(args.schema) if args.schema else None
        self._by_dir: dict[Path, Settings] = {}

    def for_file(self, path: Path) -> Settings:
        directory = path.parent.resolve()
        if directory not in self._by_dir:
            self._by_dir[directory] = self._resolve(directory)
        return self._by_dir[directory]

    def _resolve(self, directory: Path) -> Settings:
        from sqlty._config import load_config
        from sqlty.core import strip_source

        config, config_dir = load_config(directory)
        args = self._args
        schema_sql = self._schema_sql
        if schema_sql is None and "schema" in config and config_dir is not None:
            schema_sql = _load_schema(config_dir / config["schema"])
        untyped = getattr(args, "untyped", None) or config.get("untyped", "skip")
        if untyped not in ("skip", "sentinel"):
            raise SystemExit(f"{config_dir}/pyproject.toml: untyped must be 'skip' or 'sentinel'")
        import_style = config.get("import_style")
        try:
            strip_source("", import_style=import_style)
        except ValueError as exc:
            raise SystemExit(
                f"{config_dir}/pyproject.toml: {exc}; use 'from sqlty import SQL [as Q]' "
                "or 'import sqlty [as sy]'"
            ) from None
        return Settings(
            dialect=args.dialect or config.get("dialect", "postgres"),
            schema_sql=schema_sql,
            import_style=import_style,
            untyped=untyped == "sentinel",
            rules=config.get("rules"),
        )


def _load_schema(schema_path: Path) -> str | None:
    """Load schema SQL from a file path."""
    if schema_path.exists():
        return schema_path.read_text(encoding="utf-8")
    logger.warning("Schema file not found: %s", schema_path)
    return None


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

"""CLI entry point — ruff-style subcommand router.

Commands:
    sqlty rewrite [OPTIONS] [PATH...]   Rewrite source files
    sqlty strip [OPTIONS] [PATH...]     Strip SQL[T](...) wrappers
    sqlty check [OPTIONS] [PATH...]     Lint SQL type annotations
    sqlty version                       Print version
"""

from __future__ import annotations

import logging
import sys
from pathlib import Path
from typing import TYPE_CHECKING, NamedTuple

if TYPE_CHECKING:
    import argparse

    from sqlty._config import SQLTyConfig

logger = logging.getLogger(__name__)


def main(argv: list[str] | None = None) -> None:
    """CLI entry point."""
    import argparse

    if argv is None:
        argv = sys.argv[1:]

    parser = argparse.ArgumentParser(
        prog="sqlty",
        description="Type-safe SQL queries for Python.",
    )
    parser.add_argument("--version", action="store_true", help="Print version and exit")

    subparsers = parser.add_subparsers(dest="command")

    # --- rewrite ---
    rewrite_parser = subparsers.add_parser(
        "rewrite",
        help='Rewrite "--sql ..." strings into SQL[T]("--sql ...") calls',
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
        help='Strip SQL[T]("--sql ...") wrappers back to plain "--sql ..." strings',
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
        help="Auto-fix fixable issues (expand SELECT *, update stale types)",
    )

    # --- version ---
    subparsers.add_parser("version", help="Print version and exit")

    args = parser.parse_args(argv)

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
        case "version":
            _cmd_version()
        case _:
            parser.print_help()


def _add_common_args(parser: argparse.ArgumentParser) -> None:
    """Add arguments shared across rewrite/strip/check."""
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        default=[Path(".")],
        help="Files or directories to process (default: current directory)",
    )
    parser.add_argument(
        "--dialect",
        default="postgres",
        choices=["postgres", "sqlite", "mysql"],
        help="SQL dialect (default: postgres)",
    )
    parser.add_argument(
        "--schema",
        type=Path,
        help="Path to schema .sql file for enhanced inference",
    )
    parser.add_argument("-v", "--verbose", action="count", default=0, help="Increase verbosity")
    parser.add_argument("-q", "--quiet", action="store_true", help="Suppress non-error output")


# --- Command implementations ---


def _cmd_rewrite(args: argparse.Namespace) -> None:
    """Rewrite command: transform --sql strings into SQL[T](...) calls."""
    from sqlty.core import rewrite_source

    files = _files_with_settings(args)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path, settings in files:
        original = path.read_text(encoding="utf-8")
        rewritten = rewrite_source(
            original,
            dialect=settings.dialect,
            schema_sql=settings.schema_sql,
            import_style=settings.import_style,
            untyped=settings.untyped == "sentinel",
        )

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

    files = _files_with_settings(args)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path, settings in files:
        original = path.read_text(encoding="utf-8")
        stripped = strip_source(original, import_style=settings.import_style)

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
    from sqlty.core import check_source

    files = _files_with_settings(args)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    total_errors = 0
    total_warnings = 0

    for path, settings in files:
        source = path.read_text(encoding="utf-8")
        diagnostics = check_source(
            source,
            dialect=settings.dialect,
            schema_sql=settings.schema_sql,
            rules=settings.rules,
            import_style=settings.import_style,
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

    if not args.quiet and (total_errors or total_warnings):
        parts = []
        if total_errors:
            parts.append(f"{total_errors} error(s)")
        if total_warnings:
            parts.append(f"{total_warnings} warning(s)")
        print(f"Found {', '.join(parts)}")

    if total_errors > 0:
        sys.exit(1)


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


class _Settings(NamedTuple):
    dialect: str
    schema_sql: str | None
    import_style: str | None
    rules: dict[str, str] | None
    untyped: str


def _files_with_settings(args: argparse.Namespace) -> list[tuple[Path, _Settings]]:
    """Pair each Python file under ``args.paths`` with the settings that apply to it.

    Each file uses the ``[tool.sqlty]`` table of the ``pyproject.toml`` nearest
    to it, so ``sqlty rewrite proj_a proj_b`` applies each project's schema,
    dialect, import style and rules to its own files.
    """
    from sqlty._config import load_config

    resolved: dict[Path | None, _Settings] = {}
    result: list[tuple[Path, _Settings]] = []
    for path in _collect_python_files(args.paths):
        config, config_dir = load_config(path.parent)
        if config_dir not in resolved:
            resolved[config_dir] = _resolve_config(args, config, config_dir)
        result.append((path, resolved[config_dir]))
    return result


def _resolve_config(
    args: argparse.Namespace, config: SQLTyConfig, config_dir: Path | None
) -> _Settings:
    """Merge a ``[tool.sqlty]`` table found in ``config_dir`` with CLI flags.

    CLI flags (--dialect, --schema, --untyped) override pyproject.toml values;
    a config ``schema`` is relative to ``config_dir``.
    """
    # Dialect: CLI flag overrides config (argparse default is "postgres")
    dialect = args.dialect
    if dialect == "postgres" and "dialect" in config:
        # Only use config value if CLI wasn't explicitly set (still default)
        dialect = config["dialect"]

    schema_sql: str | None = None
    if args.schema:
        schema_sql = _load_schema(args.schema)
    elif "schema" in config and config_dir is not None:
        schema_sql = _load_schema(config_dir / config["schema"])

    return _Settings(
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=config.get("import_style"),
        rules=config.get("rules"),
        untyped=getattr(args, "untyped", None) or config.get("untyped", "skip"),
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

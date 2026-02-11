"""CLI entry point — ruff-style subcommand router.

Commands:
    sqlty rewrite [OPTIONS] [PATH...]   Rewrite source files
    sqlty strip [OPTIONS] [PATH...]     Strip SQL[T](...) wrappers
    sqlty check [OPTIONS] [PATH...]     Lint SQL type annotations
    sqlty version                       Print version
"""

import logging
import sys
from pathlib import Path

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
    parser.add_argument(
        "--version", action="store_true", help="Print version and exit"
    )

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


def _add_common_args(parser: "argparse.ArgumentParser") -> None:
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
    parser.add_argument(
        "-v", "--verbose", action="count", default=0, help="Increase verbosity"
    )
    parser.add_argument(
        "-q", "--quiet", action="store_true", help="Suppress non-error output"
    )


# --- Command implementations ---


def _cmd_rewrite(args: "argparse.Namespace") -> None:
    """Rewrite command: transform --sql strings into SQL[T](...) calls."""
    from sqlty.core import rewrite_source

    dialect, schema_sql = _resolve_config(args)
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path in files:
        original = path.read_text(encoding="utf-8")
        rewritten = rewrite_source(original, dialect=dialect, schema_sql=schema_sql)

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


def _cmd_strip(args: "argparse.Namespace") -> None:
    """Strip command: remove SQL[T](...) wrappers."""
    from sqlty.core import strip_source

    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path in files:
        original = path.read_text(encoding="utf-8")
        stripped = strip_source(original)

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


def _cmd_check(args: "argparse.Namespace") -> None:
    """Check command: lint SQL type annotations."""
    from sqlty.core import check_source

    dialect, schema_sql = _resolve_config(args)
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    # Load rule config from pyproject.toml [tool.sqlty.rules]
    rules = _load_rules(args.paths[0] if args.paths else None)

    total_errors = 0
    total_warnings = 0

    for path in files:
        source = path.read_text(encoding="utf-8")
        diagnostics = check_source(
            source, dialect=dialect, schema_sql=schema_sql, rules=rules
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


def _resolve_config(args: "argparse.Namespace") -> tuple[str, str | None]:
    """Resolve dialect and schema from pyproject.toml [tool.sqlty] merged with CLI args.

    CLI flags (--dialect, --schema) override pyproject.toml values.

    Returns:
        Tuple of (dialect, schema_sql content or None)
    """
    from sqlty._config import load_config

    search_path = args.paths[0] if args.paths else None
    config, config_dir = load_config(search_path)

    # Dialect: CLI flag overrides config (argparse default is "postgres")
    dialect = args.dialect
    if dialect == "postgres" and "dialect" in config:
        # Only use config value if CLI wasn't explicitly set (still default)
        dialect = config["dialect"]

    # Schema: CLI --schema flag overrides config
    schema_sql: str | None = None
    if args.schema:
        schema_sql = _load_schema(args.schema)
    elif "schema" in config and config_dir is not None:
        schema_path = config_dir / config["schema"]
        schema_sql = _load_schema(schema_path)

    return dialect, schema_sql


def _load_schema(schema_path: Path) -> str | None:
    """Load schema SQL from a file path."""
    if schema_path.exists():
        return schema_path.read_text(encoding="utf-8")
    logger.warning("Schema file not found: %s", schema_path)
    return None


def _load_rules(search_path: Path | None) -> dict[str, str] | None:
    """Load rule configuration from pyproject.toml [tool.sqlty.rules]."""
    from sqlty._config import load_config

    config, _ = load_config(search_path)
    return config.get("rules")


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

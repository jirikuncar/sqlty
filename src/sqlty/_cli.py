"""CLI entry point — ruff-style subcommand router.

Commands:
    sqlty rewrite [OPTIONS] [PATH...]   Rewrite source files
    sqlty strip [OPTIONS] [PATH...]     Strip SQL[T](...) wrappers
    sqlty check [OPTIONS] [PATH...]     Lint SQL type annotations
    sqlty generate [OPTIONS] <SOURCE>   Generate .pyi stubs (legacy)
    sqlty version                       Print version

Backward compatibility:
    sqlty <source> [--check] [--schema ...]
    routes to `generate` when the first positional arg is a path (not a command).
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

    # Backward compatibility: if first arg is a path (not a known command),
    # route to legacy generate. This handles `sqlty . --check`, `sqlty queries.py`, etc.
    _known_commands = {"rewrite", "strip", "check", "generate", "version"}
    if argv and argv[0] not in _known_commands and not argv[0].startswith("-"):
        _cmd_generate_legacy(argv)
        return

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

    # --- generate (legacy stub generation) ---
    generate_parser = subparsers.add_parser(
        "generate",
        help="Generate .pyi stub files (legacy command)",
    )
    generate_parser.add_argument("source", type=Path, help="Source file or directory")
    generate_parser.add_argument("-o", "--output", type=Path, help="Output stub file path")
    generate_parser.add_argument(
        "--dialect", default="postgres", choices=["postgres", "sqlite", "mysql"]
    )
    generate_parser.add_argument("--schema", action="append", type=str)
    generate_parser.add_argument("--mode", type=str)
    generate_parser.add_argument("--format-command", type=str)
    generate_parser.add_argument(
        "--check", action="store_true", help="Check stubs without modifying"
    )
    generate_parser.add_argument("-v", "--verbose", action="count", default=0)

    # --- version ---
    subparsers.add_parser("version", help="Print version and exit")

    # Parse - handle backward compatibility
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
        case "generate":
            _cmd_generate(args)
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

    schema_sql = _load_schema(args.schema) if args.schema else None
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    changed = 0
    for path in files:
        original = path.read_text(encoding="utf-8")
        rewritten = rewrite_source(original, dialect=args.dialect, schema_sql=schema_sql)

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

    schema_sql = _load_schema(args.schema) if args.schema else None
    files = _collect_python_files(args.paths)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    # Load rule config from pyproject.toml
    rules = _load_rules(args.paths[0] if args.paths else None)

    total_errors = 0
    total_warnings = 0

    for path in files:
        source = path.read_text(encoding="utf-8")
        diagnostics = check_source(
            source, dialect=args.dialect, schema_sql=schema_sql, rules=rules
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


def _cmd_generate(args: "argparse.Namespace") -> None:
    """Legacy generate command."""
    _run_generate(
        source=args.source,
        output=args.output,
        dialect=args.dialect,
        schema=args.schema,
        mode=args.mode,
        format_command=args.format_command,
        check=args.check,
        verbose=args.verbose,
    )


def _cmd_generate_legacy(argv: list[str]) -> None:
    """Backward-compatible legacy invocation: sqlty <source> [options]."""
    import argparse

    parser = argparse.ArgumentParser(
        prog="sqlty",
        description="Generate .pyi stub files with overloads from SQL function calls.",
    )
    parser.add_argument("source", type=Path)
    parser.add_argument("-o", "--output", type=Path)
    parser.add_argument(
        "--dialect", default="postgres", choices=["postgres", "sqlite", "mysql"]
    )
    parser.add_argument("--schema", action="append", type=str)
    parser.add_argument("--mode", type=str)
    parser.add_argument("--format-command", type=str)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("-v", "--verbose", action="count", default=0)

    args = parser.parse_args(argv)
    _run_generate(
        source=args.source,
        output=args.output,
        dialect=args.dialect,
        schema=args.schema,
        mode=args.mode,
        format_command=args.format_command,
        check=args.check,
        verbose=args.verbose,
    )


def _run_generate(
    *,
    source: Path,
    output: Path | None,
    dialect: str,
    schema: list[str] | None,
    mode: str | None,
    format_command: str | None,
    check: bool,
    verbose: int,
) -> None:
    """Shared logic for generate command and legacy invocation."""
    from sqlty._config import load_config, merge_config
    from sqlty._generator import TypeStubGenerator

    config, config_dir = load_config(source.parent if source.is_file() else source)

    cli_config: dict[str, object] = {}
    if dialect != "postgres":
        cli_config["dialect"] = dialect
    if format_command:
        cli_config["format_command"] = format_command

    merged_config = merge_config(config, cli_config)
    sql_dialect = merged_config.get("dialect", dialect)
    fmt_command = merged_config.get("format_command", format_command)

    if check and fmt_command:
        logger.error("--check and --format-command are mutually exclusive")
        sys.exit(1)

    if mode is None:
        mode = "incremental,prune,create" if source.is_dir() else "incremental,create"

    valid_actions = {"incremental", "prune", "create"}
    mode_actions = {m.strip() for m in mode.split(",")}
    invalid_actions = mode_actions - valid_actions
    if invalid_actions:
        logger.error("Invalid mode action(s): %s", ", ".join(invalid_actions))
        sys.exit(1)

    schema_mappings: dict[str | None, Path] = {}
    base_schema: Path | None = None

    if "schemas" in merged_config:
        for registry_name, schema_path_str in merged_config["schemas"].items():
            try:
                resolved = (
                    config_dir / schema_path_str if config_dir else Path(schema_path_str)
                )
                if registry_name == "":
                    base_schema = resolved
                else:
                    schema_mappings[registry_name] = resolved
            except Exception:
                logger.warning("Invalid schema path in config: %s", schema_path_str)

    if schema:
        for entry in schema:
            if ":" in entry:
                registry_name, schema_path_str = entry.split(":", 1)
                registry_name = registry_name.strip()
                schema_path_str = schema_path_str.strip()
                if not registry_name or not schema_path_str:
                    logger.error("Invalid --schema mapping: %s", entry)
                    sys.exit(1)
                if "." not in registry_name:
                    logger.error(
                        "Invalid --schema mapping: '%s' is not fully qualified", entry
                    )
                    sys.exit(1)
                schema_mappings[registry_name] = Path(schema_path_str)
            else:
                base_schema = Path(entry)

    if base_schema is not None:
        schema_mappings = dict(schema_mappings)
        schema_mappings[None] = base_schema

    function_configs = merged_config.get("functions", [])

    generator = TypeStubGenerator(
        sql_dialect=sql_dialect,
        schema_mappings=schema_mappings,
        format_command=fmt_command,
        verbose=verbose,
        function_configs=function_configs,
    )

    try:
        if check:
            exit_code = generator.check(source_path=source, output_path=output)
            sys.exit(exit_code)
        else:
            generator.generate_stub_file(
                source_path=source, output_path=output, mode=mode
            )
    except Exception as e:
        logger.error("%s", e)
        sys.exit(1)


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


def _load_schema(schema_path: Path) -> str | None:
    """Load schema SQL from a file path."""
    if schema_path.exists():
        return schema_path.read_text(encoding="utf-8")
    logger.warning("Schema file not found: %s", schema_path)
    return None


def _load_rules(search_path: Path | None) -> dict[str, str] | None:
    """Load rule configuration from pyproject.toml."""
    from sqlty._config import load_config

    config, _ = load_config(search_path)
    # The config may have 'rules' under tool.sqlty.rules in the raw toml
    # but our load_config doesn't parse it yet. For now, return None.
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

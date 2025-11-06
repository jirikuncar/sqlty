import logging
import sys
from pathlib import Path

from sqlty._generator import TypeStubGenerator


def main(argv: list[str] | None = None) -> None:
    """CLI entry point for stub file generation."""
    import argparse

    parser = argparse.ArgumentParser(
        description="Generate .pyi stub files with overloads from SQL function calls."
    )
    parser.add_argument(
        "source",
        type=Path,
        help="Source file or directory to analyze",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        help="Output stub file path (default: source.pyi or directory/__init__.pyi)",
    )
    parser.add_argument(
        "--dialect",
        default="postgres",
        choices=["postgres", "sqlite", "mysql"],
        help="SQL dialect for parsing (default: postgres)",
    )
    parser.add_argument(
        "--schema",
        action="append",
        type=str,
        help=(
            "Schema specification. Either a single path applying to all registries, "
            "or a registry-specific mapping in the form registry_name:path/to/schema.sql "
            "or module.ClassName:path/to/schema.sql for fully qualified names. "
            "Can be provided multiple times for multiple mappings."
        ),
    )
    parser.add_argument(
        "--mode",
        type=str,
        help=(
            "Stub generation mode (comma-separated actions):\n"
            "  - incremental: Add new overloads to existing stubs\n"
            "  - prune: Remove overloads no longer in source\n"
            "  - create: Only create missing stubs (skip if exists)\n"
            "Examples: 'incremental', 'incremental,prune', 'incremental,prune,create'\n"
            "Default: 'incremental,create' (single file), 'incremental,prune,create' (directory)"
        ),
    )
    parser.add_argument(
        "--format-command",
        type=str,
        help="Shell command to format generated stubs. Use {filename} as placeholder for the output file path.",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Check if stub files are up-to-date without modifying them. Exits with code 1 if differences found.",
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="count",
        default=0,
        help="Increase verbosity level. Use -v for basic diff output, -vv for colored diff with rich library.",
    )

    args = parser.parse_args(argv)

    # Validate mutually exclusive options
    if args.check and args.format_command:
        logging.getLogger(__name__).error(
            "Error: --check and --format-command are mutually exclusive. "
            "Check mode compares overload content, not formatted output."
        )
        sys.exit(1)

    # Determine the mode with smart defaults
    if args.mode:
        mode = args.mode
    else:
        # Default mode detection based on source type
        mode = "incremental,prune,create" if args.source.is_dir() else "incremental,create"

    # Validate mode string
    valid_actions = {"incremental", "prune", "create"}
    mode_actions = {m.strip() for m in mode.split(",")}
    invalid_actions = mode_actions - valid_actions
    if invalid_actions:
        logging.getLogger(__name__).error(
            "Invalid mode action(s): %s. Valid actions: %s",
            ", ".join(invalid_actions),
            ", ".join(valid_actions),
        )
        sys.exit(1)

    # Parse unified --schema options into base schema and mappings
    schema_mappings: dict[str | None, Path] = {}
    base_schema: Path | None = None
    if args.schema:
        for entry in args.schema:
            # Registry-specific mapping using ':' delimiter
            if ":" in entry:
                registry_name, schema_path_str = entry.split(":", 1)
                registry_name = registry_name.strip()
                schema_path_str = schema_path_str.strip()
                if not registry_name or not schema_path_str:
                    logging.getLogger(__name__).error("Invalid --schema mapping: %s", entry)
                    logging.getLogger(__name__).error(
                        "Expected format: module.ClassName:path/to/schema.sql"
                    )
                    sys.exit(1)
                # Enforce fully qualified registry names (module.ClassName)
                if "." not in registry_name:
                    logging.getLogger(__name__).error(
                        "Invalid --schema mapping: '%s' is not fully qualified. Use module.ClassName:path",
                        entry,
                    )
                    sys.exit(1)
                schema_mappings[registry_name] = Path(schema_path_str)
            else:
                # Single schema path applying to all registries
                try:
                    base_schema = Path(entry)
                except Exception:
                    logging.getLogger(__name__).error("Invalid --schema path: %s", entry)
                    sys.exit(1)

    # Merge base schema into mappings under key None for unified handling
    if base_schema is not None:
        schema_mappings = dict(schema_mappings)
        schema_mappings[None] = base_schema

    generator = TypeStubGenerator(
        sql_dialect=args.dialect,
        schema_mappings=schema_mappings,
        format_command=args.format_command,
        verbose=args.verbose,
    )

    try:
        if args.check:
            # Use dedicated check method
            exit_code = generator.check(
                source_path=args.source,
                output_path=args.output,
            )
            sys.exit(exit_code)
        else:
            # Normal generation mode
            generator.generate_stub_file(
                source_path=args.source,
                output_path=args.output,
                mode=mode,
            )
    except Exception as e:
        logging.getLogger(__name__).error("%s", e)
        sys.exit(1)

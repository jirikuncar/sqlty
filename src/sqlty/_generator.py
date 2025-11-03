#!/usr/bin/env python3
"""
AST-based generator for .pyi stub files with overloads.

This script analyzes Python source files to find function calls with literal
string arguments and generates type stub files with @overload declarations
based on the inferred return types from usage context.

Main orchestrator that delegates to specialized modules for:
- Collection: Finding SQL calls in source code
- AST Building: Constructing Python AST nodes
- Unparsing: Converting AST to formatted Python code
- Merging: Incremental stub file updates
- Registry Detection: Finding SQLRegistry subclasses
"""

import ast
import logging
import sys
from pathlib import Path

from sqlty._sql_analyzer import SQLAnalyzer

from ._ast_builder import StubASTBuilder
from ._ast_unparser import StubASTUnparser
from ._collector import CallInfo, LiteralCallCollector
from ._registry import RegistryDetector
from ._stub_merger import StubMerger


class TypeStubGenerator:
    """Generates .pyi stub files with overload declarations."""

    def __init__(
        self,
        sql_dialect: str = "postgres",
        schema_mappings: dict[str | None, Path] | None = None,
        format_command: str | None = None,
    ) -> None:
        self.function_name = "sql"  # Always use 'sql' as the method name
        self.sql_dialect = sql_dialect
        self.schema_mappings = (
            schema_mappings or {}
        )  # Registry name -> schema path mappings (None key allowed for global)
        self.sql_analyzer = SQLAnalyzer(dialect=sql_dialect)
        self.last_import_source: str | None = None
        self.last_is_registry_method: bool = False
        # Cache analyzers per registry for performance
        self._analyzer_cache: dict[str, SQLAnalyzer] = {}
        self._format_command = format_command

        # Initialize helper components
        self._ast_builder = StubASTBuilder(function_name=self.function_name)
        self._ast_unparser = StubASTUnparser()
        self._stub_merger = StubMerger(function_name=self.function_name)
        self._registry_detector = RegistryDetector()

    def _format_stub_text(self, text: str, filename: str) -> str:
        """Format generated stub content by piping it through an external command.

        The configured format command must use the {filename} placeholder, which will be
        replaced with the provided filename. The command is executed with the stub content
        provided on stdin, and the formatted content is read from stdout.

        If no formatter is configured or the formatter fails, the original text
        is returned unchanged.
        """
        if not self._format_command:
            return text

        cmd = self._format_command.format(filename=filename)
        try:
            import subprocess as sp

            result = sp.run(
                cmd,
                shell=True,
                input=text.encode("utf-8"),
                capture_output=True,
                timeout=30,
            )
            if result.returncode != 0:
                # Include both stdout and stderr to aid debugging
                logging.getLogger(__name__).warning(
                    "format_command failed (exit %s) for '%s'\n%s%s",
                    result.returncode,
                    cmd,
                    result.stdout.decode("utf-8", errors="ignore"),
                    result.stderr.decode("utf-8", errors="ignore"),
                )
                return text
            return result.stdout.decode("utf-8", errors="ignore")
        except FileNotFoundError:
            logging.getLogger(__name__).warning(
                "Formatter executable not found when running: %s", cmd
            )
            return text
        except Exception as e:
            logging.getLogger(__name__).warning("Could not run formatter '%s': %s", cmd, e)
            return text

    def _get_analyzer_for_registry(
        self, registry_name: str | None, module_path: Path
    ) -> SQLAnalyzer:
        """Get the appropriate SQLAnalyzer for a registry.

        Priority:
        1. Registry-specific schema from schema_mappings
        2. Global CLI schema from schema_mappings[None]
        3. __schema_path__ from registry class

        Args:
            registry_name: Name of the registry module
            module_path: Path to the module file

        Returns:
            Appropriate SQLAnalyzer instance
        """
        # Check if we already have a cached analyzer for this registry
        cache_key = registry_name or str(module_path)
        if cache_key in self._analyzer_cache:
            return self._analyzer_cache[cache_key]

        schema_to_use: Path | None = None
        schema_source = "default"

        # Priority 1: Registry-specific schema from mappings
        if registry_name and registry_name in self.schema_mappings:
            schema_to_use = self.schema_mappings[registry_name]
            schema_source = f"mapping for '{registry_name}'"
        # Priority 2: Default CLI schema provided via schema_mappings[None]
        elif None in self.schema_mappings:
            schema_to_use = self.schema_mappings[None]
            schema_source = "CLI --schema"
        # Priority 3: __schema_path__ from registry class (only if no CLI schema)
        else:
            detected_schema = self._registry_detector.get_schema_path_from_registry(module_path)
            if detected_schema:
                schema_to_use = detected_schema
                schema_source = "__schema_path__"

        # Create analyzer
        analyzer = SQLAnalyzer(dialect=self.sql_dialect, schema_path=schema_to_use)

        # Cache it
        self._analyzer_cache[cache_key] = analyzer

        # Log schema usage
        if schema_to_use:
            logging.getLogger(__name__).warning(
                "Using schema from %s (%s)", schema_to_use, schema_source
            )

        return analyzer

    # Delegation methods for backward compatibility with tests
    def _get_registry_class_name(self, module_path: Path) -> str | None:
        """Get the name of the SQLRegistry subclass in a module."""
        return self._registry_detector.get_registry_class_name(module_path)

    def _get_registry_export_name(self, module_path: Path, class_name: str) -> str:
        """Get the exported name for the registry's sql method."""
        return self._registry_detector.get_registry_export_name(module_path, class_name)

    def _get_schema_path_from_registry(self, module_path: Path) -> Path | None:
        """Extract __schema_path__ from a SQLRegistry subclass."""
        return self._registry_detector.get_schema_path_from_registry(module_path)

    def _resolve_import_to_registry_module(
        self, import_source: str, source_file: Path
    ) -> str | None:
        """Resolve an import to find which module contains the SQLRegistry subclass."""
        return self._registry_detector.resolve_import_to_registry_module(import_source, source_file)

    # Delegation method for merging (used by tests)
    def merge_stub_incremental(
        self,
        output_path: Path,
        new_calls: list[CallInfo],
        class_name: str = "Query",
        base_class: str = "SQLRegistry",
        full_mode: bool = False,
        export_name: str = "sql",
    ) -> str:
        """Merge new calls with existing stub file."""
        return self._stub_merger.merge_stub_incremental(
            output_path, new_calls, class_name, base_class, full_mode, export_name
        )

    def analyze_file(self, filepath: Path) -> list[CallInfo]:
        """Analyze a single Python file and collect function calls."""
        try:
            with open(filepath) as f:
                source = f.read()
            tree = ast.parse(source, filename=str(filepath))
        except Exception as e:
            logging.getLogger(__name__).warning("Failed to parse %s: %s", filepath, e)
            return []
        # First pass: collect calls
        collector = LiteralCallCollector()
        collector.visit(tree)

        # Resolve each import source to find modules with SQLRegistry subclass
        # Also check if the current file itself has a SQLRegistry subclass
        registry_sources: set[str] = set()

        # Check if current file has a registry
        if self._registry_detector.get_registry_class_name(filepath):
            # Use the module name (filename without .py)
            current_module = filepath.stem
            registry_sources.add(current_module)

        # Resolve imported sources
        for _, module_name in collector.import_sources.items():
            registry_module = self._registry_detector.resolve_import_to_registry_module(
                module_name, filepath
            )
            if registry_module:
                registry_sources.add(registry_module)
                # Update calls that use this import with the registry source
                for call in collector.calls:
                    if call["registry_source"] == module_name:
                        call["registry_source"] = registry_module

        # For calls without a registry_source, check if they're in the current file's registry
        current_module = filepath.stem
        if current_module in registry_sources:
            for call in collector.calls:
                if call["registry_source"] is None:
                    call["registry_source"] = current_module

        # Store the first registry source for single-file mode
        if registry_sources:
            self.last_import_source = list(registry_sources)[0]
            self.last_is_registry_method = True
        else:
            self.last_import_source = None
            self.last_is_registry_method = False

        calls = collector.calls
        # Attach file path to each call for provenance (used in comments)
        for c in calls:
            if not c.get("file"):
                c["file"] = str(filepath)

        # Infer types from SQL query analysis (with registry-specific analyzers)
        self._infer_types_from_sql(calls, filepath)

        return calls

    def _infer_types_from_sql(self, calls: list[CallInfo], module_path: Path) -> None:
        """Infer return types by analyzing SQL queries using sqlglot.

        Args:
            calls: List of call information to analyze
            module_path: Path to the module containing these calls
        """
        for call in calls:
            literal = call["literal"]
            registry_name = call.get("registry_source")

            # Get the appropriate analyzer for this registry
            analyzer = self._get_analyzer_for_registry(registry_name, module_path)

            sql_type = analyzer.generate_return_type(literal)
            if sql_type:
                call["return_type"] = f"SQL[{sql_type}]"
            else:
                # Warn about missing type information
                file_info = f" in {call.get('file', 'unknown')}" if call.get("file") else ""
                line_info = f":{call.get('lineno', '?')}" if call.get("lineno") else ""
                query_preview = literal[:60] + "..." if len(literal) > 60 else literal
                logging.getLogger(__name__).warning(
                    "Unable to infer type for query%s%s. Consider adding explicit type casts (e.g., column::TYPE). Query: %s",
                    file_info,
                    line_info,
                    query_preview,
                )

    def analyze_directory(self, dirpath: Path) -> list[CallInfo]:
        """Recursively analyze all Python files in a directory."""
        all_calls: list[CallInfo] = []
        import_sources: set[str] = set()

        for filepath in dirpath.rglob("*.py"):
            if filepath.name.startswith("_"):
                continue
            try:
                calls = self.analyze_file(filepath)
                for call in calls:
                    call["file"] = str(filepath)
                all_calls.extend(calls)

                # Track import sources
                if self.last_import_source:
                    import_sources.add(self.last_import_source)

            except Exception as e:
                logging.getLogger(__name__).warning("Failed to analyze %s: %s", filepath, e)

        # Store the most common import source for directory mode
        if import_sources:
            # For simplicity, use the first one (or we could count frequency)
            self.last_import_source = list(import_sources)[0]
            if len(import_sources) > 1:
                logging.getLogger(__name__).warning(
                    "Multiple import sources found: %s. Using: %s",
                    import_sources,
                    self.last_import_source,
                )

        return all_calls

    def generate_stub_content(
        self,
        calls: list[CallInfo],
        class_name: str = "Query",
        base_class: str = "SQLRegistry",
        export_name: str = "sql",
        relative_to: Path | None = None,
    ) -> str:
        """Generate class-based stub content from collected calls.

        This generates a Query class that inherits from SQLRegistry with
        @overload @classmethod sql() methods for each unique query literal.

        Uses AST construction and custom unparsing for robust code generation.
        """
        # Build the module AST using the AST builder
        module = self._ast_builder.build_stub_module(
            calls, class_name, base_class, export_name, relative_to
        )

        # Unparse it using custom formatter
        return self._ast_unparser.unparse_stub_module(module, calls, relative_to)

    def generate_stub_file(
        self,
        source_path: Path,
        output_path: Path | None = None,
        class_name: str = "Query",
        base_class: str = "SQLRegistry",
        mode: str = "fresh",
    ) -> Path:
        """Generate a .pyi stub file for the given source file or directory.

        Args:
            source_path: Path to source file or directory to analyze
            output_path: Path for the generated stub file
            class_name: Name of the Query class (used as fallback)
            base_class: Name of the base class
            mode: Generation mode - "fresh", "incremental", "full", or "prune"
                - fresh: Create new stub (default)
                - incremental: Add new overloads to existing stub
                - full: Regenerate all overloads (default for directory)
                - prune: Remove overloads no longer in source
        """
        # Compute internal flags from mode
        incremental = mode == "incremental"
        is_full_mode = mode in ("full", "prune")
        prune_mode = mode == "prune"

        # Check if source exists
        if not source_path.exists():
            raise FileNotFoundError(f"Source path does not exist: {source_path}")

        if source_path.is_file():
            calls = self.analyze_file(source_path)
        else:
            calls = self.analyze_directory(source_path)
            # Directory mode implies full mode for cleanup
            if mode == "fresh":
                is_full_mode = True

        if not calls:
            logging.getLogger(__name__).info(
                "No calls to %s found in %s", self.function_name, source_path
            )
            # Always create a minimal stub file
            stub_path = output_path or source_path.with_suffix(".pyi")
            stub_path.parent.mkdir(parents=True, exist_ok=True)
            minimal_stub = (
                "from typing import overload, Literal\n"
                f"from sqlty import SQL, {base_class}\n\n\n"
                f"class {class_name}({base_class}):\n"
                '    """Application-specific SQL query registry with type-safe overloads."""\n    '
                f"    @overload\n    @classmethod\n    def {self.function_name}[T](cls, query: T) -> T: ...\n\n"
                f"# Type alias for the sql function\n{self.function_name} = {class_name}.{self.function_name}\n"
            )
            # Format via pipe if configured, then write
            formatted = self._format_stub_text(minimal_stub, str(stub_path))
            with open(stub_path, "w") as f:
                f.write(formatted)
            return stub_path

        # Group calls by registry_source
        calls_by_registry: dict[str | None, list[CallInfo]] = {}
        for call in calls:
            registry = call["registry_source"]
            if registry not in calls_by_registry:
                calls_by_registry[registry] = []
            calls_by_registry[registry].append(call)

        # Generate stub files for each registry
        generated_paths: list[Path] = []
        for registry_source, registry_calls in calls_by_registry.items():
            if registry_source and not output_path:
                # Auto-detected registry: determine output path based on registry source
                if source_path.is_file():
                    stub_path = source_path.parent / f"{registry_source}.pyi"
                else:
                    stub_path = source_path / f"{registry_source}.pyi"
                logging.getLogger(__name__).info(
                    "Detected SQLRegistry subclass in '%s', writing to %s",
                    registry_source,
                    stub_path,
                )
                # Get the actual class name and export name from the registry module
                if source_path.is_file():
                    registry_file = source_path.parent / f"{registry_source}.py"
                else:
                    registry_file = source_path / f"{registry_source}.py"
                actual_class_name = self._registry_detector.get_registry_class_name(registry_file)
                if not actual_class_name:
                    actual_class_name = class_name
                export_name = self._registry_detector.get_registry_export_name(
                    registry_file, actual_class_name
                )
            elif output_path:
                # Explicit output path provided: use it
                stub_path = output_path
                actual_class_name = class_name
                export_name = self.function_name
            elif source_path.is_file():
                # No registry detected, no explicit output: use source file name
                stub_path = source_path.with_suffix(".pyi")
                actual_class_name = class_name
                export_name = self.function_name
            else:
                # Directory without explicit output: use __init__.pyi
                stub_path = source_path / "__init__.pyi"
                actual_class_name = class_name
                export_name = self.function_name

            # Generate the stub content with merging logic
            if incremental or (is_full_mode and stub_path.exists()):
                stub_content = self._stub_merger.merge_stub_incremental(
                    stub_path,
                    registry_calls,
                    actual_class_name,
                    base_class,
                    is_full_mode,
                    export_name,
                    prune=prune_mode,
                )
            else:
                # Fresh generation
                stub_content = self.generate_stub_content(
                    registry_calls,
                    actual_class_name,
                    base_class,
                    export_name,
                    relative_to=stub_path.parent,
                )

            # Write formatted output to file
            stub_path.parent.mkdir(parents=True, exist_ok=True)
            formatted = self._format_stub_text(stub_content, str(stub_path))
            with open(stub_path, "w") as f:
                f.write(formatted)

            mode_desc = "full" if is_full_mode else ("incremental" if incremental else "fresh")
            print(f"Generated stub file ({mode_desc}): {stub_path}")
            print(f"Found {len(registry_calls)} call(s) to {self.function_name}")
            generated_paths.append(stub_path)

        # Return the first generated path (or the primary one)
        return (
            generated_paths[0]
            if generated_paths
            else (output_path or source_path.with_suffix(".pyi"))
        )


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
            "or a registry-specific mapping in the form registry_name:path/to/schema.sql. "
            "Can be provided multiple times for multiple mappings."
        ),
    )
    parser.add_argument(
        "--mode",
        type=str,
        choices=["incremental", "full", "prune"],
        help="Stub generation mode: incremental (add new overloads), full (regenerate all), prune (remove unused overloads)",
    )
    parser.add_argument(
        "--format-command",
        type=str,
        help="Shell command to format generated stubs. Use {filename} as placeholder for the output file path.",
    )

    args = parser.parse_args(argv)

    # Determine the mode string from flags
    mode = args.mode or "fresh"

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
                        "Expected format: registry_name:path/to/schema.sql"
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
    )

    try:
        generator.generate_stub_file(
            source_path=args.source,
            output_path=args.output,
            mode=mode,
        )

        # TODO: Implement prune logic when mode="prune"
        # This will require extending StubMerger to track and remove unused overloads
        if mode == "prune":
            logging.getLogger(__name__).info(
                "Prune mode enabled but not yet implemented; stubs generated in full mode"
            )
    except Exception as e:
        logging.getLogger(__name__).error("%s", e)
        sys.exit(1)

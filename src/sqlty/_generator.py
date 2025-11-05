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
import difflib
import io
import logging
import os
import sys
from pathlib import Path
from typing import TYPE_CHECKING

from sqlty._formatter import formatter
from sqlty._sql_analyzer import SQLAnalyzer

from ._collector import CallInfo, LiteralCallCollector
from ._registry import RegistryDetector
from ._stub_merger import StubMerger

if TYPE_CHECKING:
    from ._stub_ir import StubFile


class TypeStubGenerator:
    """Generates .pyi stub files with overload declarations."""

    def __init__(
        self,
        sql_dialect: str = "postgres",
        schema_mappings: dict[str | None, Path] | None = None,
        format_command: str | None = None,
        check_mode: bool = False,
        verbose: int = 0,
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
        self._check_mode = check_mode
        self._verbose = verbose

        self._stub_merger = StubMerger(function_name=self.function_name)
        self._registry_detector = RegistryDetector()

        # Track files that have differences (for check mode)
        self._files_with_differences: list[tuple[Path, str, str]] = []

    def check(
        self,
        source_path: Path,
        output_path: Path | None = None,
        mode: str = "incremental,prune,create",
    ) -> int:
        """Check if stub files are up-to-date without modifying them.

        This method generates the expected stub IRs and compares them with
        existing stub files, reporting differences in a compact format.

        Args:
            source_path: Path to source file or directory to analyze
            output_path: Optional output path for stub file
            mode: Stub generation mode

        Returns:
            0 if all stubs are up-to-date, 1 if differences found
        """
        # Temporarily enable check mode for generation
        original_check_mode = self._check_mode
        self._check_mode = True

        # Clear any previous differences
        self._files_with_differences.clear()

        try:
            # Generate stubs (in check mode, this collects IRs without writing)
            self.generate_stub_file(
                source_path=source_path,
                output_path=output_path,
                mode=mode,
            )

        finally:
            self._check_mode = original_check_mode

        # Report results
        return self._report_ir_check_results()

    def _report_ir_check_results(self) -> int:
        """Report check results using IR-level diff.

        Returns 0 if no differences found, 1 if differences exist.
        """
        if not self._files_with_differences:
            print("✓ All stub files are up to date")
            return 0

        from ._stub_ir import StubFile
        from ._stub_parser import PyiToIRParser

        parser = PyiToIRParser(self.function_name)

        print(f"✗ Found {len(self._files_with_differences)} file(s) with differences:")

        for stub_path, existing_text, expected_text in self._files_with_differences:
            print(f"\n  {stub_path}")

            # Parse existing and expected stubs to IR
            if existing_text:
                # Parse from text (we already have it serialized)
                import ast

                existing_tree = ast.parse(existing_text)
                existing_ir = parser.parse_ast(existing_tree, stub_path)
            else:
                existing_ir = StubFile(file_path=stub_path)

            expected_tree = ast.parse(expected_text)
            expected_ir = parser.parse_ast(expected_tree, stub_path)

            # Compute IR-level diff
            diff = existing_ir.compute_overload_diff(expected_ir)

            # Display compact diff
            for registry_name, changes in diff.items():
                if changes["missing"] or changes["extra"]:
                    print(f"    Registry '{registry_name}':")

                    if changes["missing"]:
                        print(f"      - {len(changes['missing'])} overload(s) removed:")
                        for query, return_type in changes["missing"][:3]:  # Show first 3
                            query_preview = query[:60] + "..." if len(query) > 60 else query
                            print(f"        • {query_preview} → {return_type}")
                        if len(changes["missing"]) > 3:
                            print(f"        ... and {len(changes['missing']) - 3} more")

                    if changes["extra"]:
                        print(f"      + {len(changes['extra'])} overload(s) added:")
                        for query, return_type in changes["extra"][:3]:  # Show first 3
                            query_preview = query[:60] + "..." if len(query) > 60 else query
                            print(f"        • {query_preview} → {return_type}")
                        if len(changes["extra"]) > 3:
                            print(f"        ... and {len(changes['extra']) - 3} more")

            # Show traditional diff in verbose mode
            if self._verbose >= 1:
                print("\n    Detailed diff:")
                diff_output = self._render_diff(stub_path, existing_text, expected_text)
                if diff_output:
                    # Indent the diff
                    for line in diff_output.split("\n"):
                        print(f"    {line}")

        if self._verbose == 0:
            print("\nRun with --verbose to see detailed text diffs.")

        return 1

    def _write_stub_ir_if_changed(self, stub_path: Path, stub_ir: "StubFile") -> bool:
        """Write stub file from IR only if content has changed.

        In check mode, compares IRs instead of text to avoid false positives
        from formatting differences.

        Args:
            stub_path: Path where stub file should be written
            stub_ir: Intermediate representation of the stub file

        Returns:
            True if file was written (changed), False if unchanged.
        """
        if self._check_mode:
            # In check mode, compare IRs
            from ._stub_ir import StubFile
            from ._stub_parser import PyiToIRParser

            # Parse existing stub to IR if it exists
            if stub_path.exists():
                parser = PyiToIRParser(self.function_name)
                existing_ir = parser.parse_file(stub_path)
            else:
                # No existing file - create empty IR
                existing_ir = StubFile(file_path=stub_path)

            # Compare using the StubFile method
            if existing_ir.has_same_overloads(stub_ir):
                # No differences
                return False

            # Differences found - serialize for diff display
            from ._stub_serializer import IRToTextSerializer

            serializer = IRToTextSerializer(self.function_name)

            existing_text = (
                serializer.serialize_to_text(existing_ir, relative_to=stub_path.parent)
                if stub_path.exists()
                else ""
            )
            new_text = serializer.serialize_to_text(stub_ir, relative_to=stub_path.parent)

            # IRs differ, record for reporting
            self._files_with_differences.append((stub_path, existing_text, new_text))
            return True
        else:
            # Normal mode: serialize and write
            from ._stub_serializer import IRToTextSerializer

            serializer = IRToTextSerializer(self.function_name)
            content = serializer.serialize_to_text(stub_ir, relative_to=stub_path.parent)

            # Format if needed (but not in check mode)
            formatted = (
                formatter(self._format_command, content, str(stub_path))
                if self._format_command
                else content
            )

            # Check if content changed
            if stub_path.exists():
                try:
                    existing_content = stub_path.read_text(encoding="utf-8")
                    if existing_content == formatted:
                        return False
                except Exception:
                    pass

            # Write the file
            stub_path.write_text(formatted, encoding="utf-8")
            return True

    def _render_diff(self, filepath: Path, old_content: str, new_content: str) -> str:
        """Render a unified diff between old and new content.

        Uses rich library for colored output if verbose mode is enabled.
        """
        old_lines = old_content.splitlines(keepends=True)
        new_lines = new_content.splitlines(keepends=True)

        diff_lines = list(
            difflib.unified_diff(
                old_lines,
                new_lines,
                fromfile=str(filepath) + " (existing)",
                tofile=str(filepath) + " (expected)",
                lineterm="",
            )
        )

        if not diff_lines:
            return ""

        # For verbose level 2+, use rich for syntax highlighting
        if self._verbose >= 2:
            try:
                from rich.console import Console
                from rich.syntax import Syntax

                # Join diff lines back into a string
                diff_text = "\n".join(diff_lines)

                # Create a string buffer to capture rich output
                buffer = io.StringIO()
                console = Console(file=buffer, force_terminal=True, width=120)

                # Use Syntax for diff highlighting
                syntax = Syntax(diff_text, "diff", theme="monokai", line_numbers=False)
                console.print(syntax)

                return buffer.getvalue()
            except ImportError:
                # Fall back to plain diff if rich is not available
                pass

        # Plain diff output
        return "\n".join(diff_lines)

    def _write_stub_if_changed(self, stub_path: Path, content: str) -> bool:
        """Write stub file only if content has changed.

        Returns True if file was written (changed), False if unchanged.
        This prevents unnecessary file modifications that would trigger pre-commit hooks.

        In check mode, only checks for differences without writing.
        """
        # Read existing content if file exists
        existing_content = None
        if stub_path.exists():
            try:
                existing_content = stub_path.read_text(encoding="utf-8")
                if existing_content == content:
                    # Content is identical, skip writing
                    return False
            except Exception:
                # If we can't read the file, proceed with writing
                existing_content = None

        # In check mode, record differences but don't write
        if self._check_mode:
            if existing_content is None:
                existing_content = ""
            self._files_with_differences.append((stub_path, existing_content, content))
            return True

        # Write the file
        stub_path.write_text(content, encoding="utf-8")
        return True

    def _get_analyzer_for_registry(
        self,
        registry_name: str | None,
        module_path: Path,
        registry_class: str | None = None,
    ) -> SQLAnalyzer:
        """Get the appropriate SQLAnalyzer for a registry.

        Priority:
        1. Registry-specific schema from schema_mappings (supports fully qualified names)
        2. Global CLI schema from schema_mappings[None]
        3. __schema_path__ from registry class

        Args:
            registry_name: Name of the registry module (e.g., 'user_queries')
            module_path: Path to the module file
            registry_class: Name of the specific registry class (e.g., 'UserQueries').
                           If provided, only that class's __schema_path__ is used.

        Returns:
            Appropriate SQLAnalyzer instance
        """
        # Check if we already have a cached analyzer for this registry+class combination
        cache_key = f"{registry_name or str(module_path)}::{registry_class or ''}"
        if cache_key in self._analyzer_cache:
            return self._analyzer_cache[cache_key]

        schema_to_use: Path | None = None
        schema_source = "default"

        # Resolve the registry module path (the file that defines the registry)
        registry_module_path = module_path
        if registry_name:
            # Try to resolve registry_name to the actual file path
            resolved_module = self._registry_detector.resolve_import_to_registry_module(
                registry_name, module_path
            )
            if resolved_module:
                # Convert module name to file path
                registry_file = module_path.parent / f"{resolved_module}.py"
                if registry_file.exists():
                    registry_module_path = registry_file

        # Priority 1: Registry-specific schema from mappings
        # Only support fully qualified names (e.g., 'user_queries.UserQueries')
        if registry_name:
            # If registry_class is provided, use it; otherwise get the first one
            class_name: str | None
            if registry_class:
                # If registry_class is "sql", it's likely from "import sql" - resolve to actual class
                if registry_class == "sql":
                    class_name = self._registry_detector.get_class_name_from_export(
                        registry_module_path, "sql"
                    )
                    logging.getLogger(__name__).debug(
                        f"Resolved registry_class='sql' to class_name='{class_name}' from {registry_module_path}"
                    )
                else:
                    class_name = registry_class
            else:
                class_name = None

            # Fallback to first registry class if resolution failed
            if not class_name:
                class_names = self._registry_detector.get_registry_class_names(registry_module_path)
                class_name = class_names[0] if class_names else None

            if class_name:
                fqn = f"{registry_name}.{class_name}"
                if fqn in self.schema_mappings:
                    schema_to_use = self.schema_mappings[fqn]
                    schema_source = f"mapping for '{fqn}'"

        # Priority 2: Default CLI schema provided via schema_mappings[None]
        if schema_to_use is None and None in self.schema_mappings:
            schema_to_use = self.schema_mappings[None]
            schema_source = "CLI --schema"

        # Priority 3: __schema_path__ from registry class (only if no CLI schema)
        if schema_to_use is None:
            # Determine which class name to use for schema lookup:
            # - If registry_name exists, we've resolved class_name above (may differ from registry_class)
            # - If registry_name is None, registry_class contains the direct class name
            lookup_class_name = class_name if registry_name else registry_class
            detected_schema = self._registry_detector.get_schema_path_from_registry(
                registry_module_path, class_name=lookup_class_name
            )
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
    def _get_registry_class_names(self, module_path: Path) -> list[str]:
        """Get the names of all SQLRegistry subclasses in a module."""
        return self._registry_detector.get_registry_class_names(module_path)

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

    def _resolve_registry_source_for_class(
        self,
        call: CallInfo,
        collector: LiteralCallCollector,
        registry_sources: set[str],
    ) -> None:
        """Resolve registry_source for a call based on its registry_class.

        Args:
            call: The call info to update
            collector: The collector with import information
            registry_sources: Set of known registry module names
        """
        if call["registry_source"] is None and call["registry_class"]:
            # Check if registry_class is an imported name
            registry_class = call["registry_class"]
            if registry_class in collector.import_sources:
                import_module = collector.import_sources[registry_class]
                # Check if this module is in registry_sources (i.e., it has a SQLRegistry)
                if import_module in registry_sources:
                    call["registry_source"] = import_module

    def analyze_file(self, filepath: Path) -> list[CallInfo]:
        """Analyze a single Python file and collect function calls."""
        logger = logging.getLogger(__name__)
        logger.debug("Analyzing file: %s", filepath)

        try:
            with open(filepath) as f:
                source = f.read()
            tree = ast.parse(source, filename=str(filepath))
        except Exception as e:
            logger.warning("Failed to parse %s: %s", filepath, e)
            return []
        # First pass: collect calls
        collector = LiteralCallCollector()
        collector.visit(tree)

        logger.debug("Found %d calls in %s", len(collector.calls), filepath)
        logger.debug("Import sources: %s", collector.import_sources)
        logger.debug("Import levels: %s", collector.import_levels)

        # Resolve each import source to find modules with SQLRegistry subclass
        # Also check if the current file itself has a SQLRegistry subclass
        registry_sources: set[str] = set()

        # Check if current file has a registry
        if self._registry_detector.get_registry_class_names(filepath):
            # Use the module name (filename without .py)
            current_module = filepath.stem
            registry_sources.add(current_module)
            logger.debug("Current file has registry: %s", current_module)

        # Resolve imported sources
        for local_name, module_name in collector.import_sources.items():
            import_level = collector.import_levels.get(local_name, 0)
            logger.debug(
                "Resolving import: local_name=%s, module_name=%s, level=%d",
                local_name,
                module_name,
                import_level,
            )
            resolved_module = self._registry_detector.resolve_import_to_registry_module(
                module_name, filepath, import_level
            )
            if resolved_module:
                registry_sources.add(resolved_module)
                logger.debug("Resolved to registry source: %s", resolved_module)

                # Update calls that use this import with the resolved registry source
                for call in collector.calls:
                    if call["registry_source"] == module_name:
                        call["registry_source"] = resolved_module
                        logger.debug("Updated call registry_source to: %s", resolved_module)
            else:
                logger.debug("Import does not resolve to a registry")
                # If the module doesn't contain a SQLRegistry, clear registry_source for those calls
                for call in collector.calls:
                    if call["registry_source"] == module_name:
                        call["registry_source"] = None

        # For calls with registry_class but no registry_source, check if the class is imported
        for call in collector.calls:
            self._resolve_registry_source_for_class(call, collector, registry_sources)

        # For calls without a registry_source, check if they're in the current file's registry
        current_module = filepath.stem
        if current_module in registry_sources:
            for call in collector.calls:
                if call["registry_source"] is None:
                    call["registry_source"] = current_module
                    logger.debug("Set call registry_source to current module: %s", current_module)

        logger.debug("Final registry_sources: %s", registry_sources)
        logger.debug(
            "Final calls with registry_source: %s",
            [c.get("registry_source") for c in collector.calls],
        )

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
            registry_class = call.get("registry_class")

            # Get the appropriate analyzer for this registry
            analyzer = self._get_analyzer_for_registry(registry_name, module_path, registry_class)

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
            # Skip private modules but not __init__.py
            if filepath.name.startswith("_") and filepath.name != "__init__.py":
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

        Uses the new IR-based approach for better structure and maintainability.
        """
        stub_ir = self.generate_stub_ir(calls, class_name, base_class, export_name)

        from ._stub_serializer import IRToTextSerializer

        serializer = IRToTextSerializer(self.function_name)
        return serializer.serialize_to_text(stub_ir, relative_to=relative_to)

    def generate_stub_ir(
        self,
        calls: list[CallInfo],
        class_name: str = "Query",
        base_class: str = "SQLRegistry",
        export_name: str = "sql",
    ) -> "StubFile":
        """Generate stub IR from collected calls.

        Returns:
            StubFile IR representing the stub structure
        """
        from ._stub_parser import CallInfoToIRParser

        parser = CallInfoToIRParser(self.function_name)
        stub_ir = parser.parse_calls(calls, class_name, base_class, export_name)
        return stub_ir

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
            mode: Generation mode - comma-separated list of actions:
                - incremental: Add new overloads to existing stub
                - prune: Remove overloads no longer in source
                - create: Only create missing stubs (skip if exists)
                Examples: "incremental", "incremental,prune", "incremental,prune,create"
        """
        # Parse comma-separated modes into flags
        modes = {m.strip() for m in mode.split(",")}
        incremental = "incremental" in modes
        prune_mode = "prune" in modes
        create_only = "create" in modes

        # Legacy mode support (for backward compatibility)
        if mode in ("full", "fresh"):
            incremental = False
            prune_mode = False
            create_only = False

        is_directory = source_path.is_dir()

        # Helper: Check if we should skip because of create_only mode
        def should_skip_create_only(stub_path: Path) -> bool:
            """Check if we should skip processing due to create_only mode."""
            return create_only and stub_path.exists() and not incremental and not prune_mode

        # Safety check: prune should only run on directory mode
        if prune_mode and not is_directory:
            logging.getLogger(__name__).warning(
                "WARNING: Prune mode is not recommended for single file mode."
            )
            logging.getLogger(__name__).warning(
                "This may remove stubs for queries defined elsewhere in your project."
            )
            # Prompt user for confirmation
            try:
                response = input("Do you want to continue? (y/n): ").strip().lower()
                if response != "y":
                    logging.getLogger(__name__).info("Aborting prune operation.")
                    sys.exit(0)
            except (EOFError, KeyboardInterrupt):
                logging.getLogger(__name__).info("\nAborting prune operation.")
                sys.exit(0)

        # Check if source exists
        if not source_path.exists():
            raise FileNotFoundError(f"Source path does not exist: {source_path}")

        if source_path.is_file():
            calls = self.analyze_file(source_path)
        else:
            calls = self.analyze_directory(source_path)

        if not calls:
            logging.getLogger(__name__).info(
                "No calls to %s found in %s", self.function_name, source_path
            )
            # Always create a minimal stub file (unless create_only and file exists and no other operations)
            stub_path = output_path or source_path.with_suffix(".pyi")

            # Skip if create_only mode and file already exists and no other operations
            if should_skip_create_only(stub_path):
                logging.getLogger(__name__).info(
                    "Stub file already exists, skipping (create mode): %s", stub_path
                )
                return stub_path

            stub_path.parent.mkdir(parents=True, exist_ok=True)

            # Check if source file has multiple registry classes
            if source_path.is_file():
                registry_classes = self._registry_detector.get_registry_class_names(source_path)
            else:
                registry_classes = []

            # Create minimal stub IR for empty registries
            from ._stub_ir import RegistryClass, StubFile

            minimal_stub_ir = StubFile(file_path=stub_path)

            if len(registry_classes) > 1:
                # Generate minimal stubs for all registry classes
                for reg_class in registry_classes:
                    export_name = self._registry_detector.get_registry_export_name(
                        source_path, reg_class
                    )
                    registry = RegistryClass(
                        name=reg_class,
                        base_class=base_class,
                        export_name=export_name,
                        overloads=set(),  # Empty set for minimal stub
                    )
                    minimal_stub_ir.add_registry(registry)
            else:
                # Single registry class or no registry detected
                actual_class = registry_classes[0] if registry_classes else class_name
                registry = RegistryClass(
                    name=actual_class,
                    base_class=base_class,
                    export_name=self.function_name,
                    overloads=set(),  # Empty set for minimal stub
                )
                minimal_stub_ir.add_registry(registry)

            # Use IR-based writing
            self._write_stub_ir_if_changed(stub_path, minimal_stub_ir)
            return stub_path

        # Group calls by the actual registry file location (not just module name)
        # This handles cases where multiple directories have files with the same name
        calls_by_registry_file: dict[Path, list[CallInfo]] = {}

        for call in calls:
            registry_source = call["registry_source"]
            if not registry_source:
                # Skip calls that don't belong to a SQLRegistry
                logging.getLogger(__name__).info(
                    "Skipping call without registry_source: %s:%s",
                    call.get("file", "unknown file"),
                    call.get("lineno", "?"),
                )
                continue

            # Convert module name to path
            registry_source = registry_source.replace(".", os.path.sep)

            # Find the actual file that defines this registry
            registry_file: Path | None = None
            if source_path.is_file():
                registry_file = source_path.parent / f"{registry_source}.py"
            else:
                # Search for files matching the registry_source name
                # Check which one actually has a registry class defined
                candidates = list(source_path.rglob(f"{registry_source}.py"))

                # For each candidate, check if it defines a SQLRegistry subclass
                # and if THIS CALL references it (not other calls)
                call_file = call.get("file")
                if call_file:
                    call_path = Path(call_file)

                    for candidate in candidates:
                        if self._registry_detector.get_registry_class_names(candidate):
                            # If the call is from the registry file itself
                            if call_path == candidate:
                                registry_file = candidate
                                break
                            # Or if they're in the same directory (likely related)
                            if call_path.parent == candidate.parent:
                                registry_file = candidate
                                break

                # If we didn't find a match through directory relationship,
                # use the first candidate with a registry class
                if registry_file is None:
                    for candidate in candidates:
                        if self._registry_detector.get_registry_class_names(candidate):
                            registry_file = candidate
                            break

                # Final fallback
                if registry_file is None and candidates:
                    registry_file = candidates[0]
                elif registry_file is None:
                    registry_file = source_path / f"{registry_source}.py"

            if registry_file not in calls_by_registry_file:
                calls_by_registry_file[registry_file] = []
            calls_by_registry_file[registry_file].append(call)

        # Generate stub files for each registry file
        generated_paths: list[Path] = []
        for registry_file, registry_calls in calls_by_registry_file.items():
            if not output_path:
                stub_path = registry_file.with_suffix(".pyi")
                registry_source = registry_file.stem
                logging.getLogger(__name__).info(
                    "Detected SQLRegistry subclass in '%s', writing to %s",
                    registry_source,
                    stub_path,
                )
                class_names = self._registry_detector.get_registry_class_names(registry_file)

                # Unified path: handle one or multiple classes the same way
                if class_names:
                    # Resolve registry_class names that might be export aliases
                    for call in registry_calls:
                        registry_class = call.get("registry_class")
                        if registry_class and registry_class not in class_names:
                            # This might be an export alias (e.g., 'qa' instead of 'A')
                            # Try to resolve it to the actual class name
                            resolved_class = self._registry_detector.get_class_name_from_export(
                                registry_file, registry_class
                            )
                            if resolved_class and resolved_class in class_names:
                                call["registry_class"] = resolved_class

                    # Group calls by registry_class
                    calls_by_class: dict[str | None, list[CallInfo]] = {}
                    for call in registry_calls:
                        registry_class = call.get("registry_class")
                        if registry_class not in calls_by_class:
                            calls_by_class[registry_class] = []
                        calls_by_class[registry_class].append(call)

                    # If there's only one registry class in the module, assign unclassified calls to it
                    if len(class_names) == 1 and None in calls_by_class:
                        only_cls = class_names[0]
                        calls_by_class[only_cls] = calls_by_class.get(
                            only_cls, []
                        ) + calls_by_class.pop(None)

                    # Handle incremental/full mode for multiple classes
                    # Handle all modes using IR-based merge
                    # Parse all new registries first
                    from ._stub_ir import StubFile
                    from ._stub_parser import CallInfoToIRParser

                    call_parser = CallInfoToIRParser(self.function_name)

                    # Create stub with all new registries
                    all_new_registries = StubFile(file_path=stub_path)
                    for cls_name in class_names:
                        cls_calls = calls_by_class.get(cls_name, [])
                        export_name = self._registry_detector.get_registry_export_name(
                            registry_file, cls_name
                        )
                        new_stub = call_parser.parse_calls(
                            cls_calls, cls_name, base_class, export_name
                        )
                        for registry in new_stub.registries.values():
                            all_new_registries.add_registry(registry)

                    # Merge with existing stub if it exists
                    if stub_path.exists():
                        # Skip if create_only mode and no other operations requested
                        if should_skip_create_only(stub_path):
                            logging.getLogger(__name__).info(
                                "Stub file already exists, skipping (create mode): %s", stub_path
                            )
                            generated_paths.append(stub_path)
                            continue

                        from ._stub_parser import PyiToIRParser

                        pyi_parser = PyiToIRParser(self.function_name)
                        existing_stub = pyi_parser.parse_file(stub_path)

                        # Apply modes in order: incremental first, then prune
                        for new_registry in all_new_registries.registries.values():
                            if incremental:
                                # Add new overloads
                                existing_stub.merge_registry(new_registry, mode="incremental")
                            if prune_mode:
                                # Remove stale overloads
                                existing_stub.merge_registry(new_registry, mode="prune")
                            if not incremental and not prune_mode:
                                # Fresh mode: replace entirely
                                existing_stub.merge_registry(new_registry, mode="full")

                        # Use IR-based writing for better check mode support
                        final_stub_ir = existing_stub
                    else:
                        # Use IR-based writing for better check mode support
                        final_stub_ir = all_new_registries

                    self._write_stub_ir_if_changed(stub_path, final_stub_ir)

                    generated_paths.append(stub_path)
                    mode_parts = []
                    if incremental:
                        mode_parts.append("incremental")
                    if prune_mode:
                        mode_parts.append("prune")
                    if create_only:
                        mode_parts.append("create")
                    mode_label = ",".join(mode_parts) if mode_parts else "fresh"

                    logging.getLogger(__name__).info(
                        "Generated stub file (%s): %s", mode_label, stub_path
                    )
                    logging.getLogger(__name__).info(
                        "Found %s call(s) to %s", len(registry_calls), self.function_name
                    )
                    # Mirror CLI output expectations (skip in check mode)
                    if not self._check_mode:
                        print(f"Generated stub file ({mode_label}): {stub_path}")
                        print(f"Found {len(registry_calls)} call(s) to {self.function_name}")
                    continue  # Skip the rest of the loop for this registry
                else:
                    # No registry classes found in target module; use generic single-class generation
                    actual_class_name = class_name
                    export_name = self.function_name

                    # Skip if create_only mode and file already exists and no other operations
                    if should_skip_create_only(stub_path):
                        logging.getLogger(__name__).info(
                            "Stub file already exists, skipping (create mode): %s", stub_path
                        )
                        generated_paths.append(stub_path)
                        continue

                    # Generate the stub content with merging logic
                    full_mode = False  # Use incremental/prune modes instead of full regeneration
                    if incremental and stub_path.exists():
                        stub_content = self._stub_merger.merge_stub_incremental(
                            stub_path,
                            registry_calls,
                            actual_class_name,
                            base_class,
                            full_mode,
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

            # Skip if create_only mode and file already exists and no other operations
            if should_skip_create_only(stub_path):
                logging.getLogger(__name__).info(
                    "Stub file already exists, skipping (create mode): %s", stub_path
                )
                generated_paths.append(stub_path)
                continue

            # Generate the stub content with merging logic
            full_mode = False  # Use incremental/prune modes instead of full regeneration
            if incremental and stub_path.exists():
                stub_content = self._stub_merger.merge_stub_incremental(
                    stub_path,
                    registry_calls,
                    actual_class_name,
                    base_class,
                    full_mode,
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
            formatted = (
                formatter(self._format_command, stub_content, str(stub_path))
                if self._format_command
                else stub_content
            )
            self._write_stub_if_changed(stub_path, formatted)

            mode_parts = []
            if incremental:
                mode_parts.append("incremental")
            if prune_mode:
                mode_parts.append("prune")
            if create_only:
                mode_parts.append("create")
            mode_desc = ",".join(mode_parts) if mode_parts else "fresh"
            if not self._check_mode:
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
    valid_actions = {"incremental", "prune", "create", "full", "fresh"}
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
        check_mode=False,  # Check mode is now handled by .check() method
        verbose=args.verbose,
    )

    try:
        if args.check:
            # Use dedicated check method
            exit_code = generator.check(
                source_path=args.source,
                output_path=args.output,
                mode=mode,
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

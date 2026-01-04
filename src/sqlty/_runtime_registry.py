"""Runtime registry for tracking sql() calls during script execution.

This module provides a thread-safe registry that tracks sql() calls made during
script execution when SQLTY_AUTO_GENERATE is enabled. The registry collects
call information and serializes it to stub files at script exit.
"""

import atexit
import contextlib
import inspect
import logging
import threading
from collections import defaultdict
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


class RuntimeCallRegistry:
    """Thread-safe registry for tracking sql() calls during runtime.

    This registry collects information about sql() calls and their context,
    then generates or updates stub files when the script exits.
    """

    def __init__(self) -> None:
        """Initialize the runtime registry."""
        self._lock = threading.Lock()
        # Map: source_file -> list of call info dicts
        self._calls: dict[Path, list[dict[str, Any]]] = defaultdict(list)
        self._registered_exit = False

    def register_call(
        self,
        query: str,
        registry_class: str | None = None,
        fully_qualified_name: str | None = None,
        registry_file: str | None = None,
        module_name: str | None = None,
    ) -> None:
        """Register a sql() call from runtime.

        Args:
            query: The SQL query string
            registry_class: Name of the registry class that owns this call
            fully_qualified_name: Fully qualified name of the registry class (module.ClassName)
            registry_file: Path to the file where the registry class is defined
            module_name: Name of the module where the call was made
        """
        # Get the caller's frame to determine source location
        frame = inspect.currentframe()
        if frame is None:
            logger.debug("No frame available")
            return

        try:
            # Walk up the stack to find the actual caller
            # We need to skip:
            # 1. This method (register_call)
            # 2. The @sqlty decorated function
            # 3. Any intermediate frames
            caller_frame = frame.f_back  # sql() method
            if caller_frame is None:
                logger.debug("No caller frame (f_back is None)")
                return

            caller_frame = caller_frame.f_back  # Actual caller
            if caller_frame is None:
                logger.debug("No actual caller frame (f_back.f_back is None)")
                return

            # Get the source file and line number
            if registry_file:
                source_file = registry_file
                lineno = 1  # Default line number when registry_file is provided
            else:
                # Get the source file and line number from call stack
                source_file = caller_frame.f_code.co_filename
                lineno = caller_frame.f_lineno

            logger.debug(f"Call from {source_file}:{lineno}")

            # Convert to Path and check if it's a valid Python file
            source_path = Path(source_file)
            if not source_path.exists():
                logger.debug(f"Source file does not exist: {source_path}")
                return
            if source_path.suffix != ".py":
                logger.debug(f"Not a Python file: {source_path}")
                return

            # Create call info
            call_info: dict[str, Any] = {
                "literal": query,
                "lineno": lineno,
                "file": str(source_path),
                "registry_class": registry_class,
                "registry_source": module_name,
                "fully_qualified_name": fully_qualified_name,
                "registry_file": registry_file,
                "return_type": None,  # Will be inferred during generation
            }

            with self._lock:
                self._calls[source_path].append(call_info)
                logger.debug(f"Registered call from {source_path}:{lineno}")

                # Register atexit handler on first call
                if not self._registered_exit:
                    atexit.register(self._generate_stubs_at_exit)
                    self._registered_exit = True
                    logger.debug("Registered atexit handler for stub generation")

        finally:
            del frame

    def _generate_stubs_at_exit(self) -> None:
        """Generate or update stub files for all registered calls.

        This method is called automatically at script exit via atexit.
        It processes all collected calls and generates/updates stub files
        using the incremental mode with file locking.
        """
        if not self._calls:
            return

        logger.debug("Generating stubs at exit for %d files", len(self._calls))

        # Import here to avoid circular imports
        try:
            from sqlty._config import load_config
            from sqlty._generator import TypeStubGenerator
        except ImportError as e:
            logger.warning("Failed to import generator: %s", e)
            return

        # Load configuration from pyproject.toml
        # Use the first source file's directory as a starting point for config search
        source_paths = list(self._calls.keys())
        if source_paths:
            config, _ = load_config(source_paths[0].parent)
        else:
            config, _ = load_config()

        # Extract function configs and other settings from config
        function_configs = config.get("functions", [])
        sql_dialect = config.get("dialect", "postgres")

        generator = TypeStubGenerator(
            sql_dialect=sql_dialect,
            function_configs=function_configs,
        )

        with self._lock:
            # Process each source file
            for source_path, calls in self._calls.items():
                if not calls:
                    continue

                try:
                    self._generate_stub_for_file(source_path, calls, generator)
                except Exception as e:
                    # Don't crash the application if stub generation fails
                    logger.warning("Failed to generate stub for %s: %s", source_path, e)

    def _generate_stub_for_file(
        self,
        source_path: Path,
        calls: list[dict[str, Any]],
        generator: Any,  # TypeStubGenerator
    ) -> None:
        """Generate or update a stub file for a single source file.

        Args:
            source_path: Path to the source file
            calls: List of call information dictionaries
            generator: TypeStubGenerator instance
        """
        import fcntl

        stub_path = source_path.with_suffix(".pyi")

        # Use file locking to prevent race conditions
        # Lock the stub file for writing
        lock_path = stub_path.with_suffix(".pyi.lock")

        try:
            with open(lock_path, "w") as lock_file:
                # Acquire exclusive lock
                fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX)

                try:
                    # Enrich calls with configuration information
                    self._enrich_calls_with_config(calls, generator)

                    # Infer types from SQL for all calls
                    generator._infer_types_from_sql(calls, source_path)

                    # Get the registry class name
                    registry_classes = generator._get_registry_names(source_path)
                    if not registry_classes:
                        logger.warning("No registry class found in %s", source_path)
                        return

                    # Group calls by registry class
                    calls_by_class: dict[str | None, list[dict[str, Any]]] = defaultdict(list)
                    for call in calls:
                        reg_class = call.get("registry_class")
                        calls_by_class[reg_class].append(call)

                    # For @sqlty functions, merge all functions into one stub
                    # Check if this is a @sqlty file (all registry classes are @sqlty functions)
                    is_sqlty_file = all(
                        reg_class and reg_class in registry_classes
                        for reg_class in calls_by_class
                        if reg_class is not None
                    )

                    if is_sqlty_file and len(calls_by_class) > 1:
                        # Merge all @sqlty functions into one stub
                        all_calls = []
                        for class_calls in calls_by_class.values():
                            all_calls.extend(class_calls)

                        # Use the first registry class name for the stub
                        class_name = registry_classes[0]
                        export_name = generator._get_registry_export_name(source_path, class_name)

                        # Generate merged stub for all functions
                        stub_content = generator.merge_stub_incremental(
                            stub_path,
                            all_calls,
                            class_name=class_name,
                            full_mode=False,
                            export_name=export_name,
                        )

                        # Write the merged stub file
                        stub_path.write_text(stub_content, encoding="utf-8")

                        logger.info(
                            "Generated merged stub for %s (%d functions, %d calls)",
                            source_path.name,
                            len(calls_by_class),
                            len(all_calls),
                        )
                    else:
                        # Generate stubs for each registry class (legacy behavior)
                        for reg_class_name, class_calls in calls_by_class.items():
                            # Determine which registry class to use
                            if reg_class_name and reg_class_name in registry_classes:
                                class_name = reg_class_name
                            else:
                                class_name = registry_classes[0]

                            # Get the export name for this registry
                            export_name = generator._get_registry_export_name(
                                source_path, class_name
                            )

                            # Use incremental mode to merge with existing stub
                            stub_content = generator.merge_stub_incremental(
                                stub_path,
                                class_calls,
                                class_name=class_name,
                                full_mode=False,
                                export_name=export_name,
                            )

                            # Write the stub file
                            stub_path.write_text(stub_content, encoding="utf-8")

                            logger.info(
                                "Generated stub for %s (%s.sql, %d calls)",
                                source_path.name,
                                export_name,
                                len(class_calls),
                            )

                finally:
                    # Release lock
                    fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)
        finally:
            # Clean up lock file
            with contextlib.suppress(Exception):
                lock_path.unlink()

    def _enrich_calls_with_config(self, calls: list[dict[str, Any]], generator: Any) -> None:
        """Enrich runtime calls with configuration information.

        This ensures that configured functions get the correct return type template
        and other settings from pyproject.toml.

        Args:
            calls: List of call information dictionaries to enrich
            generator: TypeStubGenerator instance with config
        """
        # Get function configs from the generator
        function_configs = getattr(generator, "_function_configs", None) or []

        for call in calls:
            registry_class = call.get("registry_class")

            if not registry_class:
                continue

            # Try to find a matching configuration
            # Configuration import_path format: "module.function_name"
            # We need to match this with registry_class
            for config in function_configs:
                import_path = config.get("import_path", "")
                if not import_path:
                    continue

                # Split import path to get the function name
                parts = import_path.split(".")
                if len(parts) < 2:
                    continue

                config_func_name = parts[-1]

                # Check if this config matches our call
                # Match by function name (last part of import_path)
                if config_func_name == registry_class:
                    # Enrich the call with configuration
                    call["query_param_name"] = config.get("query_param", "query")
                    call["return_type_template"] = config.get(
                        "return_type_template", "SQL[{sqlty}]"
                    )
                    call["self_type_template"] = config.get("self_type_template")
                    call["schema_path"] = config.get("schema")

                    # Add default imports if using SQL[...] template
                    if "SQL[" in call.get("return_type_template", ""):
                        imports = call.get("imports", [])
                        if not imports:
                            imports = []
                        if "from sqlty import SQL" not in imports:
                            imports.append("from sqlty import SQL")
                        call["imports"] = imports

                    break


# Global singleton registry instance
_runtime_registry: RuntimeCallRegistry | None = None
_registry_lock = threading.Lock()


def get_runtime_registry() -> RuntimeCallRegistry:
    """Get or create the global runtime registry instance.

    Returns:
        The global RuntimeCallRegistry instance
    """
    global _runtime_registry

    with _registry_lock:
        if _runtime_registry is None:
            _runtime_registry = RuntimeCallRegistry()
        return _runtime_registry

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
from types import UnionType
from typing import TYPE_CHECKING, Any, Literal, Union, get_args, get_origin

if TYPE_CHECKING:
    from sqlty._config import FunctionConfigComplete

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
        function_config: FunctionConfigComplete | None = None,
        return_type: object = None,
    ) -> None:
        """Register a sql() call from runtime.

        Args:
            query: The SQL query string
            registry_class: Name of the registry class that owns this call
            fully_qualified_name: Fully qualified name of the registry class (module.ClassName)
            registry_file: Path to the file where the registry class is defined
            module_name: Name of the module where the call was made
            function_config: The helper's ``[[tool.sqlty.functions]]`` entry
            return_type: Argument passed to the configured ``return_type_param``, used as
                the overload's type instead of inferring it (None infers)
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
                "function_config": function_config,
            }
            return_type_param = (
                function_config.get("return_type_param") if function_config else None
            )
            if return_type is not None and return_type_param:
                annotation, imports = type_expression(return_type, module_name)
                call_info["return_type_param_name"] = return_type_param
                call_info["return_type_param_value"] = annotation
                call_info["imports"] = imports

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

        # Use the first source file's directory as a starting point for config search
        source_paths = list(self._calls.keys())
        config, config_dir = load_config(source_paths[0].parent if source_paths else None)
        root = config_dir or Path.cwd()

        schema_mappings: dict[str | None, Path] = {
            name: root / path for name, path in config.get("schemas", {}).items()
        }
        if "schema" in config:
            schema_mappings[None] = root / config["schema"]

        format_command = config.get("format_command")
        generator = TypeStubGenerator(
            sql_dialect=config.get("dialect", "postgres"),
            schema_mappings=schema_mappings,
            format_command=format_command,
            function_configs=config.get("functions", []),
            untyped=config.get("untyped", "skip"),
        )

        with self._lock:
            # Process each source file
            for source_path, calls in self._calls.items():
                if not calls:
                    continue

                try:
                    self._generate_stub_for_file(source_path, calls, generator, format_command)
                except Exception as e:
                    # Don't crash the application if stub generation fails
                    logger.warning("Failed to generate stub for %s: %s", source_path, e)

    def _generate_stub_for_file(
        self,
        source_path: Path,
        calls: list[dict[str, Any]],
        generator: Any,  # TypeStubGenerator
        format_command: str | None = None,
    ) -> None:
        """Generate or update a stub file for a single source file.

        Args:
            source_path: Path to the source file
            calls: List of call information dictionaries
            generator: TypeStubGenerator instance
            format_command: ``format-command`` the stub is piped through before writing
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
                    self._enrich_calls_with_config(calls)

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

                        self._write_stub(stub_path, stub_content, format_command)

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

                            self._write_stub(stub_path, stub_content, format_command)

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

    @staticmethod
    def _write_stub(stub_path: Path, content: str, format_command: str | None) -> None:
        if format_command:
            from sqlty._formatter import formatter

            content = formatter(format_command, content, str(stub_path))
        stub_path.write_text(content, encoding="utf-8")

    @staticmethod
    def _enrich_calls_with_config(calls: list[dict[str, Any]]) -> None:
        """Copy each call's ``[[tool.sqlty.functions]]`` entry onto the call.

        The entry is the one the decorator resolved for the helper (by full
        ``module.function`` path), so helpers with the same name in different
        modules keep their own settings.
        """
        for call in calls:
            config = call.get("function_config")
            if not config:
                continue
            call["function_name"] = call.get("registry_class")
            call["query_param_name"] = config.get("query_param", "query")
            call["return_type_template"] = config.get("return_type_template", "SQL[{sqlty}]")
            call["self_type_template"] = config.get("self_type_template")
            call["schema_path"] = config.get("schema")

            imports = [*config.get("imports", []), *call.get("imports", [])]
            if "SQL[" in call["return_type_template"]:
                imports.append("from sqlty import SQL")
            call["imports"] = list(dict.fromkeys(imports))


def type_expression(tp: object, home_module: str | None = None) -> tuple[str, list[str]]:
    """Stub annotation for a runtime type, and the imports it needs.

    ``home_module`` is the module the stub is written for: its own classes are
    referenced without an import. Classes a stub cannot import (defined in a
    function, or in a different ``__main__`` script) become ``Any``.

    Example:
        >>> import datetime
        >>> type_expression(int)
        ('int', [])
        >>> type_expression(list[datetime.date])
        ('list[datetime.date]', ['import datetime'])
        >>> type_expression("MyRow")
        ('MyRow', [])
    """
    if isinstance(tp, str):
        return tp, []
    if tp is None or tp is type(None):
        return "None", []
    if tp is Ellipsis:
        return "...", []
    origin, args = get_origin(tp), get_args(tp)
    if origin is not None and origin is not Literal and args:
        parts = [type_expression(arg, home_module) for arg in args]
        imports = [i for _, arg_imports in parts for i in arg_imports]
        texts = [text for text, _ in parts]
        if origin in (Union, UnionType):
            return " | ".join(texts), list(dict.fromkeys(imports))
        head, head_imports = type_expression(origin, home_module)
        return f"{head}[{', '.join(texts)}]", list(dict.fromkeys(head_imports + imports))
    if isinstance(tp, type):
        module, qualname = tp.__module__, tp.__qualname__
        if module == "builtins":
            return qualname, []
        if "<" in qualname or (module == "__main__" and home_module != "__main__"):
            return "Any", ["from typing import Any"]
        if module == home_module:
            return qualname, []
        return f"{module}.{qualname}", [f"import {module}"]
    text = repr(tp)
    return text, ["import typing"] if text.startswith("typing.") else []


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

"""SQLTy - Type-safe SQL queries for Python with automatic stub generation."""

import importlib.metadata
import os
from collections.abc import Callable
from typing import TypeForm, get_args, overload

try:
    __version__ = importlib.metadata.version(__name__)
except importlib.metadata.PackageNotFoundError:
    __version__ = "0.0.0"  # Fallback for development mode

__all__ = ["SQL", "row_form", "sqlty"]


# Determine if auto-generation is enabled via environment variable
_auto_generate = os.environ.get("SQLTY_AUTO_GENERATE", "").lower() in {"1", "true", "yes"}


class SQL[T](str):
    pass


_MISSING: object = object()


@overload
def row_form[T](query: SQL[T], /) -> TypeForm[T]: ...


@overload
def row_form[T, D](query: SQL[T], default: D, /) -> TypeForm[T] | D: ...


def row_form(query: str, default: object = _MISSING, /) -> object:
    """Return the row type ``T`` a ``SQL[T](...)`` instance was built with.

    The type is read from ``__orig_class__``, so it exists only for queries
    constructed as ``SQL[T](...)``. Plain strings, annotation-only
    ``q: SQL[T] = "..."`` and stripped sources carry no runtime type: these
    raise ``TypeError`` unless a ``default`` is given.

    Example:
        >>> row_form(SQL[tuple[int, str]]("--sql\\nSELECT 1, 'a'"))
        tuple[int, str]
        >>> row_form(SQL("--sql\\nSELECT 1"), None) is None
        True
    """
    orig = getattr(query, "__orig_class__", None)
    if orig is not None and (args := get_args(orig)):
        return args[0]
    if default is not _MISSING:
        return default
    raise TypeError(f"{type(query).__name__} has no runtime row type; construct it as SQL[T](...)")


@overload
def sqlty[F](func: F, /) -> F:
    """Decorator to mark functions for SQLTy type inference and stub generation.

    Usage:
        @sqlty
        def sql(query: Literal) -> Any:
            return query
    """
    ...


@overload
def sqlty[F](*, schema: str | None = None) -> Callable[[F], F]:
    """Decorator to mark functions for SQLTy type inference and stub generation.

    Usage:
        @sqlty(schema="schema.sql")
        def sql(query: Literal) -> Any:
            return query
    """
    ...


def sqlty[F](func: F | None = None, *, schema: str | None = None) -> F | Callable[[F], F]:
    """Decorator to mark functions for SQLTy type inference and stub generation.

    Usage:
        # Simple decoration without schema
        @sqlty
        def sql(query: Literal) -> Any:
            return query

        # Decoration with schema path
        @sqlty(schema="schema.sql")
        def sql_with_schema(query: Literal) -> Any:
            return query

    When _auto_generate is enabled:
    - Verifies the decorated function is registered in pyproject.toml
    - Keeps automatic generation of stub files
    """

    def decorator(fn: F) -> F:
        # Mark the function as a sqlty function
        if hasattr(fn, "__sqlty__"):
            raise TypeError("Function is already decorated with @sqlty")

        fn.__sqlty__ = True  # type: ignore[attr-defined]  # ty: ignore[invalid-assignment]
        # Store schema path if provided
        if schema is not None:
            fn.__schema_path__ = schema  # type: ignore[attr-defined]  # ty: ignore[invalid-assignment]

        # When auto-generate is enabled, verify registration and enable runtime stub generation
        if _auto_generate:
            import functools
            import inspect
            import warnings
            from pathlib import Path
            from typing import Any, cast

            from sqlty._auto_generator import auto_generate_stub
            from sqlty._config import load_config

            # Get the module and file path of the decorated function
            is_registered = False
            try:
                module = inspect.getmodule(fn)
                if module:
                    module_name = module.__name__
                    file_path = inspect.getfile(cast(Any, fn))
                    function_name = getattr(fn, "__name__", None)

                    if function_name:
                        # Load config from pyproject.toml
                        config, _ = load_config(Path(file_path).parent)

                        # Check if this function is registered in pyproject.toml
                        # Expected format: [tool.sqlty.functions] with import_path
                        functions = config.get("functions", [])
                        expected_import_path = f"{module_name}.{function_name}"

                        is_registered = any(
                            func_config["import_path"] == expected_import_path
                            for func_config in functions
                        )

                        # If not registered and module is __main__, also check using file basename
                        # This handles the case when a script is run directly (python script.py)
                        if not is_registered and module_name == "__main__":
                            file_basename = Path(file_path).stem
                            fallback_import_path = f"{file_basename}.{function_name}"
                            is_registered = any(
                                func_config["import_path"] == fallback_import_path
                                for func_config in functions
                            )
                            if is_registered:
                                expected_import_path = fallback_import_path

                        if not is_registered:
                            warnings.warn(
                                f"Function {expected_import_path} is decorated with @sqlty but not "
                                f"registered in pyproject.toml under [tool.sqlty.functions]. "
                                f"Add this configuration:\n\n"
                                f"[[tool.sqlty.functions]]\n"
                                f'import_path = "{expected_import_path}"\n'
                                f'query_param = "query"\n',
                                stacklevel=2,
                            )
            except Exception:
                # Silently ignore errors during registration check
                pass

            # Enable runtime stub generation for registered functions
            if is_registered:
                call = functools.partial(auto_generate_stub, fn)  # type: ignore

                @functools.wraps(fn)  # type: ignore
                def wrapper(query: str) -> Any:
                    # TODO use query_param name from config or inspect signature and allow automatic detection for only one param
                    call(query)
                    return fn(query)  # type: ignore

                # Copy over the attributes to the wrapper
                wrapper.__sqlty__ = True  # type: ignore[attr-defined]  # ty: ignore[unresolved-attribute]
                if schema is not None:
                    wrapper.__schema_path__ = schema  # type: ignore[attr-defined]  # ty: ignore[unresolved-attribute]

                return wrapper  # type: ignore

        return fn

    # Handle both @sqlty and @sqlty(...) usage
    if func is not None:
        # Called as @sqlty (without parentheses)
        return decorator(func)
    else:
        # Called as @sqlty(...) (with parentheses)
        return decorator

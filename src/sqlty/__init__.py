"""SQLTy - Type-safe SQL queries for Python with automatic stub generation."""

import importlib.metadata
import os
from collections.abc import Callable
from typing import TypeForm, get_args, overload

try:
    __version__ = importlib.metadata.version(__name__)
except importlib.metadata.PackageNotFoundError:
    __version__ = "0.0.0"  # Fallback for development mode

__all__ = ["SQL", "UNTYPED", "row_form", "sqlty"]


# Determine if auto-generation is enabled via environment variable
_auto_generate = os.environ.get("SQLTY_AUTO_GENERATE", "").lower() in {"1", "true", "yes"}


class SQL[T](str):
    pass


UNTYPED = sentinel("UNTYPED")
"""Row type of a query sqlty could not infer: ``SQL[UNTYPED]``.

Unlike ``Any``, type checkers reject using an ``UNTYPED`` row as a concrete
type. ``sqlty rewrite --untyped sentinel`` emits it, as do generated stubs with
``untyped = "sentinel"`` (including their catch-all overload); ``sqlty check`` flags it
(``untyped-query``) until casts or a schema make the row type known.
"""


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

        # Async helpers get async def overloads
        @sqlty
        async def fetch(query: Literal) -> Any: ...

    When _auto_generate is enabled:
    - Verifies the decorated function is registered in pyproject.toml
    - Wraps it: each call binds its arguments to the signature (defaults applied),
      records the ``query_param`` and ``return_type_param`` values for the stub,
      and passes all arguments through unchanged
    - Writes the stub files at interpreter exit

    Otherwise the function is returned unchanged.
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
            from sqlty._config import expand_schema, load_config

            # Get the module and file path of the decorated function
            func_config = None
            try:
                module = inspect.getmodule(fn)
                if module:
                    module_name = module.__name__
                    file_path = inspect.getfile(cast(Any, fn))
                    function_name = getattr(fn, "__name__", None)

                    if function_name:
                        # Load config from pyproject.toml
                        config, config_dir = load_config(Path(file_path).parent)

                        # Check if this function is registered in pyproject.toml
                        # Expected format: [tool.sqlty.functions] with import_path
                        functions = config.get("functions", [])
                        expected_import_path = f"{module_name}.{function_name}"
                        candidates = [expected_import_path]
                        # When a script is run directly (python script.py), also accept its basename
                        if module_name == "__main__":
                            candidates.append(f"{Path(file_path).stem}.{function_name}")

                        func_config = next(
                            (
                                fc
                                for path in candidates
                                for fc in functions
                                if fc["import_path"] == path
                            ),
                            None,
                        )

                        if func_config is not None and "schema" in func_config:
                            func_config = func_config.copy()
                            root = config_dir or Path.cwd()
                            func_config["schema"] = [
                                str(p) for p in expand_schema(func_config["schema"], root)
                            ]
                        if func_config is None:
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
            if func_config is not None:
                signature = inspect.signature(cast(Any, fn))
                query_param = func_config["query_param"]
                return_type_param = func_config.get("return_type_param")
                resolved_config = func_config

                @functools.wraps(fn)  # type: ignore
                def wrapper(*args: Any, **kwargs: Any) -> Any:
                    """Record the call for stub generation, then call the helper unchanged.

                    Arguments are bound to the helper's signature (defaults applied) to read
                    ``query_param`` and ``return_type_param``; calls that do not bind, or
                    whose query is not a ``str``, are not recorded.
                    """
                    try:
                        bound = signature.bind(*args, **kwargs)
                    except TypeError:
                        pass
                    else:
                        bound.apply_defaults()
                        query = bound.arguments.get(query_param)
                        if isinstance(query, str):
                            auto_generate_stub(
                                cast(Any, fn),
                                query,
                                resolved_config,
                                bound.arguments.get(return_type_param)
                                if return_type_param
                                else None,
                            )
                    return fn(*args, **kwargs)  # type: ignore

                if inspect.iscoroutinefunction(fn):
                    inspect.markcoroutinefunction(wrapper)

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

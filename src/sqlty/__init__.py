"""SQLTy - Type-safe SQL queries for Python with automatic stub generation."""

import importlib.metadata
import os
from collections.abc import Callable
from typing import Any, overload

try:
    __version__ = importlib.metadata.version(__name__)
except importlib.metadata.PackageNotFoundError:
    __version__ = "0.0.0"  # Fallback for development mode

__all__ = ["SQL", "sqlty", "magic"]


# Determine if auto-generation is enabled via environment variable
_auto_generate = os.environ.get("SQLTY_AUTO_GENERATE", "").lower() in {"1", "true", "yes"}


class SQL[T](str):
    pass


# TODO make sure F is (query: str) -> Any


@overload
def sqlty[F](func: F, /) -> F:
    """Decorator to mark functions for SQLTy type inference and stub generation.

    Usage:
        @sqlty
        def sql[T](query: T) -> T:
            return query
    """
    ...


@overload
def sqlty[F](*, schema: str | None = None) -> Callable[[F], F]:
    """Decorator to mark functions for SQLTy type inference and stub generation.

    Usage:
        @sqlty(schema="schema.sql")
        def sql[T](query: T) -> T:
            return query
    """
    ...


def sqlty[F](func: F | None = None, *, schema: str | None = None) -> F | Callable[[F], F]:
    """Decorator to mark functions for SQLTy type inference and stub generation.

    Usage:
        # Simple decoration without schema
        @sqlty
        def sql[T](query: T) -> T:
            return query

        # Decoration with schema path
        @sqlty(schema="schema.sql")
        def sql_with_schema[T](query: T) -> T:
            return query
    """

    def decorator(fn: F) -> F:
        # Mark the function as a sqlty function
        if hasattr(fn, "__sqlty__"):
            raise TypeError("Function is already decorated with @sqlty")

        fn.__sqlty__ = True  # type: ignore[attr-defined]
        # Store schema path if provided
        if schema is not None:
            fn.__schema_path__ = schema  # type: ignore[attr-defined]

        # Auto-generate stub if environment variable is set
        if _auto_generate:
            import functools

            from sqlty._auto_generator import auto_generate_stub

            # Store the original function's file before wrapping
            call = functools.partial(auto_generate_stub, fn)  # type: ignore

            @functools.wraps(fn)  # type: ignore
            def wrapper(query: str) -> Any:
                call(query)
                return fn(query)  # type: ignore

            return wrapper  # type: ignore

        return fn

    # Handle both @sqlty and @sqlty(...) usage
    if func is not None:
        # Called as @sqlty (without parentheses)
        return decorator(func)
    else:
        # Called as @sqlty(...) (with parentheses)
        return decorator

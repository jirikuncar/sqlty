"""SQLTy - Type-safe SQL queries for Python with automatic stub generation."""

import importlib.metadata
from collections.abc import Callable
from typing import overload

try:
    __version__ = importlib.metadata.version(__name__)
except importlib.metadata.PackageNotFoundError:
    __version__ = "0.0.0"  # Fallback for development mode

__all__ = ["SQL", "SQLRegistry", "sqlty"]


class SQL[T](str):
    pass


class SQLRegistry:
    """Base class for SQL query registries with type-safe query construction.

    Subclasses can define @overload methods for specific SQL queries,
    allowing type-safe query definitions with proper return type inference.

    Automatic stub generation can be enabled by setting the SQLTY_AUTO_GENERATE
    environment variable to '1' or 'true'. When enabled, stub files will be
    automatically generated or updated when a SQLRegistry subclass is defined.
    """

    def __init_subclass__(cls, **kwargs: object) -> None:
        """Hook called when a subclass is created.

        If SQLTY_AUTO_GENERATE environment variable is set, this will
        automatically generate or update the stub file for the module.
        """
        super().__init_subclass__(**kwargs)

        import os

        auto_generate = os.environ.get("SQLTY_AUTO_GENERATE", "").lower() in ("1", "true", "yes")

        if auto_generate:
            # Delegate to helper to avoid bloating the base class and to prevent
            # circular imports across modules.
            from sqlty._auto_generator import auto_generate_stub

            auto_generate_stub(cls)

    @classmethod
    def sql[T](cls, query: T) -> T:
        """Create a typed SQL value from a literal string.

        This base implementation returns the query as-is at runtime.
        Subclasses should define @overload methods in their stub files
        to provide specific type information for known queries.
        """
        return query


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

    This decorator can be used with or without parameters:
    - @sqlty: Simple decoration without schema
    - @sqlty(schema="schema.sql"): Decoration with schema path

    The decorated function will have:
    - __sqlty__: Set to True to mark it as a SQLTy function
    - __schema_path__: Set to the schema path (if provided)

    Example:
        @sqlty
        def sql[T](query: T) -> T:
            return query

        @sqlty(schema="schema.sql")
        def sql_with_schema[T](query: T) -> T:
            return query
    """

    def decorator(fn: F) -> F:
        # Mark the function as a sqlty function
        fn.__sqlty__ = True  # type: ignore[attr-defined]
        # Store schema path if provided
        if schema is not None:
            fn.__schema_path__ = schema  # type: ignore[attr-defined]
        return fn

    # Handle both @sqlty and @sqlty(...) usage
    if func is not None:
        # Called as @sqlty (without parentheses)
        return decorator(func)
    else:
        # Called as @sqlty(...) (with parentheses)
        return decorator

"""SQLTy - Type-safe SQL queries for Python with automatic stub generation."""

import importlib.metadata

try:
    __version__ = importlib.metadata.version(__name__)
except importlib.metadata.PackageNotFoundError:
    __version__ = "0.0.0"  # Fallback for development mode

__all__ = ["SQL", "SQLRegistry"]


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

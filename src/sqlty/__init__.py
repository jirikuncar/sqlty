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
        
        When SQLTY_AUTO_GENERATE is enabled, this method also registers
        the call for automatic stub generation at script exit.
        """
        # Fast path: check if this is even a string and if auto-gen is enabled
        # Only do expensive operations if both conditions are true
        if isinstance(query, str):
            import os
            
            # Cache the auto-generate check at the module level for performance
            # Only check once per process
            if not hasattr(cls, '_sqlty_autogen_checked'):
                cls._sqlty_autogen_checked = True
                cls._sqlty_autogen_enabled = os.environ.get("SQLTY_AUTO_GENERATE", "").lower() in ("1", "true", "yes")
            
            if cls._sqlty_autogen_enabled:
                # Import only when needed
                from sqlty._runtime_registry import get_runtime_registry
                import inspect
                
                # Get the module name for this registry class
                module = inspect.getmodule(cls)
                module_name = module.__name__ if module else None
                
                # Register the call
                registry = get_runtime_registry()
                registry.register_call(
                    query=query,
                    registry_class=cls.__name__,
                    module_name=module_name,
                )
        
        return query

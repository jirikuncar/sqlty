"""Automatic stub generation utilities.

This module contains helper functions that generate or update .pyi stub files
when @sqlty decorated functions are defined at runtime (opt-in via environment).
"""

import inspect
from collections.abc import Callable
from typing import TYPE_CHECKING, Any

from sqlty._runtime_registry import get_runtime_registry

if TYPE_CHECKING:
    from sqlty._config import FunctionConfigComplete


def auto_generate_stub(
    fn: Callable[..., Any],
    query: str,
    function_config: FunctionConfigComplete,
    return_type: object = None,
) -> None:
    """Generate or update a stub file for the module that defines `fn`.

    This function is intended to be called by @sqlty decorator
    when the SQLTY_AUTO_GENERATE environment variable is enabled.

    Args:
        fn: The decorated helper.
        query: Value of its ``query_param`` argument.
        function_config: The helper's ``[[tool.sqlty.functions]]`` entry, as resolved by
            the decorator (``schema`` already absolute).
        return_type: Value of its ``return_type_param`` argument (None infers the type).

    If a .pyi file already exists, it will be updated incrementally.
    If no .pyi file exists, stub generation is deferred to the atexit handler
    which will have collected all runtime sql() calls.
    """
    # Get the module for this registry class
    module = inspect.getmodule(fn)
    if not module:
        return  # pragma: no cover

    module_name = module.__name__

    try:
        registry_file = inspect.getfile(fn)
    except TypeError, OSError:  # pragma: no cover
        return None

    # Build fully qualified class name
    fn_name: str = getattr(fn, "__name__", "unknown")
    fully_qualified_name = f"{registry_file}.{fn_name}"

    # Register the call
    registry = get_runtime_registry()
    registry.register_call(
        query=query,
        registry_class=fn_name,
        fully_qualified_name=fully_qualified_name,
        registry_file=registry_file,
        module_name=module_name,
        function_config=function_config,
        return_type=return_type,
    )

"""Automatic stub generation utilities.

This module contains helper functions that generate or update .pyi stub files
when @sqlty decorated functions are defined at runtime (opt-in via environment).
"""

import inspect
from collections.abc import Callable
from typing import Any

from sqlty._runtime_registry import get_runtime_registry


def auto_generate_stub(fn: Callable[[str], Any], query: str) -> None:
    """Generate or update a stub file for the module that defines `fn`.

    This function is intended to be called by @sqlty decorator
    when the SQLTY_AUTO_GENERATE environment variable is enabled.

    If a .pyi file already exists, it will be updated incrementally.
    If no .pyi file exists, stub generation is deferred to the atexit handler
    which will have collected all runtime sql() calls.
    """
    # Get the module for this registry class
    module = inspect.getmodule(fn)
    if not module:
        return

    module_name = module.__name__

    try:
        registry_file = inspect.getfile(fn)
    except (TypeError, OSError):
        return None

    # Build fully qualified class name
    fully_qualified_name = f"{registry_file}.{fn.__name__}"

    # Register the call
    registry = get_runtime_registry()
    registry.register_call(
        query=query,
        registry_class=fn.__name__,
        fully_qualified_name=fully_qualified_name,
        registry_file=registry_file,
        module_name=module_name,
    )

"""Automatic stub generation utilities.

This module contains helper functions that generate or update .pyi stub files
when SQLRegistry subclasses are defined at runtime (opt-in via environment).
"""

import inspect
import logging
from collections.abc import Callable
from pathlib import Path
from typing import Any


def auto_generate_stub(fn: Callable[[str], Any]) -> None:
    """Generate or update a stub file for the module that defines `fn`.

    This function is intended to be called by @sqlty decorator
    when the SQLTY_AUTO_GENERATE environment variable is enabled.
    """
    # Get the module where this function is defined
    module = inspect.getmodule(fn)
    if not module or not hasattr(module, "__file__") or not module.__file__:
        return

    source_file = Path(module.__file__)
    if not source_file.exists():
        return

    try:
        # Import lazily to avoid circular imports
        from sqlty._generator import TypeStubGenerator

        generator = TypeStubGenerator()
        generator.generate_stub_file(
            source_file,
            class_name=fn.__name__,
            mode="incremental",  # Don't overwrite existing stubs
        )

        logging.getLogger(__name__).info("[sqlty] Auto-generated stub for %s", source_file.name)
    except Exception as e:  # pragma: no cover - best-effort logging only
        # Don't crash the application if stub generation fails
        logging.getLogger(__name__).warning("[sqlty] Failed to auto-generate stub: %s", e)

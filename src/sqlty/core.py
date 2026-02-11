"""Public core API — try Rust extension, fall back to Python.

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

When a Rust extension is compiled (via maturin), it's installed as `sqlty._core`.
This module tries that first, then falls back to the pure-Python `sqlty._core_py`.
"""

try:
    from sqlty._core import (  # type: ignore[import-not-found]
        check_source,
        expand_star,
        infer_type,
        rewrite_source,
        strip_source,
    )
except ImportError:
    from sqlty._core_py import (
        check_source,
        expand_star,
        infer_type,
        rewrite_source,
        strip_source,
    )

__all__ = [
    "check_source",
    "expand_star",
    "infer_type",
    "rewrite_source",
    "strip_source",
]

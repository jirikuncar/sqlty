"""Public core API — import-switching facade.

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

Currently uses ``sqlty._core_py`` (pure Python). When a Rust extension is
compiled via maturin/pyo3, it will be installed as ``sqlty._core`` and
this module will import from it instead. Both implementations must satisfy
the same API contract (see ``tests/test_core_conformance.py``).
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

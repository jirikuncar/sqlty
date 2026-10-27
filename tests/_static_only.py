"""pytest plugin: run the suite as if the sandbox were gone, to measure the static resolver.

    uv run pytest -p tests._static_only

Delete it with ``sandbox.py``. See plans/2026-10-27-no-exec.md.
"""

from __future__ import annotations

import pytest


def pytest_configure(config: pytest.Config) -> None:
    from sqlty._type_extractor import extract, sandbox

    def nothing(*args: object, **kwargs: object) -> None:
        return None

    def no_names(*args: object, **kwargs: object) -> dict[str, object]:
        return {}

    extract._resolve_class = sandbox._resolve_class = nothing  # type: ignore[assignment]
    sandbox._populate = no_names  # type: ignore[assignment]

"""Backend parametrisation for Python tests; Markdown tests live in ``tests._mdtest``."""

from __future__ import annotations

import types
from importlib import import_module

import pytest

from tests._mdtest.plugin import BACKENDS_KEY
from tests._mdtest.runner import BACKENDS


@pytest.fixture()
def core(request: pytest.FixtureRequest) -> types.ModuleType:
    """The backend module (``_core_py`` or ``_core``), per ``--sqlty-lib`` / ``SQLTY_LIB``."""
    return import_module(BACKENDS[request.param])


def pytest_generate_tests(metafunc: pytest.Metafunc) -> None:
    if "core" in metafunc.fixturenames:
        backends: list[str] = metafunc.config.stash[BACKENDS_KEY]
        metafunc.parametrize("core", backends, indirect=True)

"""Root conftest.py — pytest plugin for mdtest discovery and backend parametrisation."""

from __future__ import annotations

import os
import types
from pathlib import Path

import pytest

from tests.mdtest_runner import MDTestFailure, parse_mdtest_file


# ---------------------------------------------------------------------------
# CLI options
# ---------------------------------------------------------------------------


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption(
        "--update-mdtest",
        action="store_true",
        default=False,
        help="Update assertion comments in .md files with actual computed values",
    )
    parser.addoption(
        "--sqlty-lib",
        choices=("py", "rs", "both"),
        default=os.environ.get("SQLTY_LIB", "both"),
        help="Backend to test: py (Python), rs (Rust), both (default)",
    )


# ---------------------------------------------------------------------------
# Backend parametrisation: ``core`` fixture
# ---------------------------------------------------------------------------


def _load_backend(name: str) -> types.ModuleType:
    """Import the named backend module."""
    if name == "rs":
        import sqlty._core as mod

        return mod  # type: ignore[return-value]
    import sqlty._core_py as mod

    return mod  # type: ignore[return-value]


def _available_backends(config: pytest.Config) -> list[str]:
    """Determine which backends to exercise based on --sqlty-lib / SQLTY_LIB."""
    choice = config.getoption("--sqlty-lib", default="both")
    if choice in ("py", "rs"):
        return [choice]
    # "both" — include Rust only if compiled
    backends = ["py"]
    try:
        import sqlty._core  # noqa: F401

        backends.append("rs")
    except ImportError:
        pass
    return backends


def pytest_configure(config: pytest.Config) -> None:
    # Store available backends for fixture parametrisation
    config._sqlty_backends = _available_backends(config)  # type: ignore[attr-defined]


@pytest.fixture()
def core(request: pytest.FixtureRequest) -> types.ModuleType:
    """Provides the sqlty core backend module (``_core_py`` or ``_core``).

    Parametrised with ``["py"]``, ``["rs"]``, or both, depending on
    ``--sqlty-lib`` / ``SQLTY_LIB``.  Each value exposes the 5 core
    functions: ``infer_type``, ``rewrite_source``, ``strip_source``,
    ``expand_star``, ``check_source``.
    """
    name: str = request.param
    if name == "rs":
        pytest.importorskip("sqlty._core", reason="Rust backend not compiled")
    return _load_backend(name)


def pytest_generate_tests(metafunc: pytest.Metafunc) -> None:
    """Dynamically parametrise the ``core`` fixture with available backends."""
    if "core" in metafunc.fixturenames:
        backends = metafunc.config._sqlty_backends  # type: ignore[attr-defined]
        metafunc.parametrize("core", backends, indirect=True)


def pytest_collect_file(parent: pytest.Collector, file_path: Path) -> MDTestFile | None:
    if file_path.suffix == ".md" and "mdtest" in file_path.parts:
        return MDTestFile.from_parent(parent, path=file_path)
    return None


class MDTestFile(pytest.File):
    def collect(self):  # type: ignore[override]
        update = self.config.getoption("--update-mdtest", default=False)
        test_cases = parse_mdtest_file(self.path, update=update)
        for tc in test_cases:
            yield MDTestItem.from_parent(self, name=tc.name, test_case=tc)


class MDTestItem(pytest.Item):
    def __init__(self, name: str, parent: pytest.Collector, test_case: object) -> None:
        super().__init__(name, parent)
        self.test_case = test_case  # MDTestCase

    def runtest(self) -> None:
        self.test_case.run()  # type: ignore[attr-defined]

    def repr_failure(
        self,
        excinfo: pytest.ExceptionInfo[BaseException],
        style: str | None = None,
    ) -> str:
        if isinstance(excinfo.value, MDTestFailure):
            return str(excinfo.value)
        return super().repr_failure(excinfo)  # type: ignore[return-value]

    def reportinfo(self):  # type: ignore[override]
        return self.path, None, f"mdtest: {self.name}"

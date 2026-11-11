"""The standard-library table and the checks that run without importing user code."""

from __future__ import annotations

import importlib.util
import sys
import warnings
from pathlib import Path

import pytest

from sqlty._config import ConfigError, validate_tool_table
from sqlty._core import python_types
from sqlty.core import check_source

MARKER_MODULE = "import pathlib\npathlib.Path({marker!r}).write_text('imported')\nfrom typing import NewType\nUserId = NewType('UserId', int)\n"
MARKER_SOURCE = """
from typing import TypedDict
from marker_app_ids import UserId
from sqlty import SQL

class User(TypedDict):
    id: UserId

q = SQL[User]("--sql\\nSELECT id::TEXT FROM users")
"""


@pytest.fixture
def marker_module(tmp_path, monkeypatch) -> Path:
    marker = tmp_path / "marker"
    (tmp_path / "marker_app_ids.py").write_text(MARKER_MODULE.format(marker=str(marker)))
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.delitem(sys.modules, "marker_app_ids", raising=False)
    return marker


def test_application_imports_are_not_executed(marker_module) -> None:
    assert check_source(MARKER_SOURCE) == []
    assert not marker_module.exists()


def test_type_modules_allow_imports() -> None:
    """``type-modules`` is gone: ``check`` imports no module, so the key is rejected."""
    with pytest.raises(
        ConfigError, match="'type-modules' in \\[tool.sqlty\\]: removed, since `check`"
    ):
        validate_tool_table({"type-modules": ["app"]}, "pyproject.toml")


def test_checked_code_warnings_are_not_printed() -> None:
    source = """from pydantic import BaseModel
from sqlty import SQL

class User(BaseModel):
    id: int

    class Config:
        strict = True

q = SQL[User]("--sql\\nSELECT 1::INTEGER AS id")
"""
    with warnings.catch_warnings(record=True, action="always") as caught:
        assert check_source(source) == []
    assert caught == []


def _generator():
    path = Path(__file__).resolve().parent.parent / "scripts" / "generate_stdlib_types.py"
    spec = importlib.util.spec_from_file_location("generate_stdlib_types", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_every_inferred_python_type_is_in_the_table() -> None:
    """A Python type the engine infers that the table leaves out derives from nothing."""
    scalars = {name for name in python_types() if "[" not in name}
    assert scalars - _generator().PYTHON_TYPE_MAP.keys() == set()


def test_stdlib_table_matches_typeshed() -> None:
    """``stdlib_table.rs`` is what typeshed gives (``make generate-stdlib-types``)."""
    module = _generator()
    assert module.RUST_OUTPUT.read_text() == module.generate_rust(module.generate()), (
        "run `make generate-stdlib-types`"
    )

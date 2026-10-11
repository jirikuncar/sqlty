"""Stubs for ``async def`` helpers are ``async def`` overloads, so ``await`` type checks."""

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from sqlty._generator import TypeStubGenerator

PYPROJECT = '[[tool.sqlty.functions]]\nimport_path = "db.fetch"\nquery_param = "query"\n'

DB = """from typing import Any

from sqlty import sqlty


@sqlty
async def fetch(query: str) -> Any:
    return [(1,)]
"""

CALL = 'fetch("--sql\\nSELECT 1::INTEGER AS n")\n'

STUB = """from typing import Literal, overload
from sqlty import SQL


@overload
async def fetch(query: Literal['--sql\\nSELECT 1::INTEGER AS n']) -> SQL[tuple[int]]:
    ...

@overload
async def fetch(query: str) -> str:
    ...
"""

USE = """from typing import assert_type

from sqlty import SQL
from db import fetch


async def main() -> None:
    assert_type(await fetch("--sql\\nSELECT 1::INTEGER AS n"), SQL[tuple[int]])
    assert_type(await fetch("SELECT 2"), str)
"""


def test_generator(tmp_path: Path) -> None:
    source = tmp_path / "db.py"
    source.write_text(DB + CALL)
    generator = TypeStubGenerator(
        function_configs=[{"import_path": "db.fetch", "query_param": "query"}]
    )
    stub = generator.generate_stub_file(source, mode="incremental,create")
    assert stub.read_text() == STUB

    generator.generate_stub_file(source, mode="incremental")
    assert stub.read_text() == STUB


def test_runtime(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text(PYPROJECT)
    (tmp_path / "db.py").write_text(
        "import asyncio\n" + DB + f"\nasyncio.run({CALL.strip()})\n"
    )
    (tmp_path / "main.py").write_text("import db\n")
    for _ in range(2):
        result = subprocess.run(
            [sys.executable, "main.py"],
            capture_output=True,
            text=True,
            cwd=tmp_path,
            env={**os.environ, "SQLTY_AUTO_GENERATE": "1"},
        )
        assert result.returncode == 0, result.stderr
        assert "No registry class found" not in result.stderr
        assert (tmp_path / "db.pyi").read_text() == STUB


@pytest.mark.parametrize(
    "command", [["mypy", "--no-error-summary", "use.py"], ["ty", "check", "use.py"]]
)
def test_await_type_checks(tmp_path: Path, command: list[str]) -> None:
    if not shutil.which(command[0]):
        pytest.skip(f"{command[0]} not installed")
    (tmp_path / "db.py").write_text(DB)
    (tmp_path / "db.pyi").write_text(STUB)
    (tmp_path / "use.py").write_text(USE)
    result = subprocess.run(command, capture_output=True, text=True, cwd=tmp_path)
    assert result.returncode == 0, result.stdout + result.stderr

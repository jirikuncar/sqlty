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

SYNC_STUB = STUB.replace("async def", "def")


def _generator() -> TypeStubGenerator:
    return TypeStubGenerator(function_configs=[{"import_path": "db.fetch", "query_param": "query"}])


def test_generator(tmp_path: Path) -> None:
    source = tmp_path / "db.py"
    source.write_text(DB + CALL)
    generator = _generator()
    stub = generator.generate_stub_file(source, mode="incremental,create")
    assert stub.read_text() == STUB

    generator.generate_stub_file(source, mode="incremental")
    assert stub.read_text() == STUB


def test_incremental_switch(tmp_path: Path) -> None:
    source = tmp_path / "db.py"
    generator = _generator()
    for db, expected in [(DB, STUB), (DB.replace("async def", "def"), SYNC_STUB), (DB, STUB)]:
        source.write_text(db + CALL)
        assert (
            generator.generate_stub_file(source, mode="incremental,create").read_text() == expected
        )


def test_no_calls(tmp_path: Path) -> None:
    source = tmp_path / "db.py"
    source.write_text(DB)
    stub = _generator().generate_stub_file(source, mode="incremental,create").read_text()
    assert "async def fetch(query: str) -> str:" in stub


def test_no_calls_two_helpers(tmp_path: Path) -> None:
    source = tmp_path / "db.py"
    source.write_text(DB + "\n\n" + DB.split("\n\n\n")[-1].replace("async def fetch", "def other"))
    generator = TypeStubGenerator(
        function_configs=[
            {"import_path": "db.fetch", "query_param": "query"},
            {"import_path": "db.other", "query_param": "query"},
        ]
    )
    stub = generator.generate_stub_file(source, mode="incremental,create").read_text()
    assert "async def fetch(query: str) -> str:" in stub
    assert "\ndef other(query: str) -> str:" in stub


@pytest.mark.parametrize(
    "decorators",
    [
        "@sqlty(schema='schema.sql')",
        "@other\n@sqlty",
        "@sqlty\n@other",
    ],
)
def test_decorators(tmp_path: Path, decorators: str) -> None:
    (tmp_path / "schema.sql").write_text("CREATE TABLE t (id INTEGER);\n")
    source = tmp_path / "db.py"
    source.write_text("def other(fn): return fn\n\n" + DB.replace("@sqlty", decorators) + CALL)
    assert _generator().generate_stub_file(source, mode="incremental,create").read_text() == STUB


@pytest.mark.parametrize(
    ("body", "expected"),
    [
        ("    yield (1,)\n", SYNC_STUB),
        ("    async def inner():\n        yield 1\n\n    return [(1,)]\n", STUB),
    ],
    ids=["async-generator", "nested-generator"],
)
def test_async_generator(tmp_path: Path, body: str, expected: str) -> None:
    source = tmp_path / "db.py"
    source.write_text(DB.replace("    return [(1,)]\n", body) + CALL)
    assert (
        _generator().generate_stub_file(source, mode="incremental,create").read_text() == expected
    )


def test_async_method_with_same_name(tmp_path: Path) -> None:
    source = tmp_path / "db.py"
    source.write_text(
        DB.replace("async def", "def")
        + "\n\nclass Repo:\n    async def fetch(self, query: str) -> Any:\n        return []\n\n"
        + CALL
    )
    assert (
        _generator().generate_stub_file(source, mode="incremental,create").read_text() == SYNC_STUB
    )


@pytest.mark.parametrize(("existing", "status"), [(SYNC_STUB, 1), (STUB, 0)])
def test_check(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], existing: str, status: int
) -> None:
    source = tmp_path / "db.py"
    source.write_text(DB + CALL)
    (tmp_path / "db.pyi").write_text(existing)
    assert _generator().check(source) == status
    assert ("overloads should be async def" in capsys.readouterr().out) == bool(status)
    assert (tmp_path / "db.pyi").read_text() == existing


def test_runtime(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text(PYPROJECT)
    (tmp_path / "db.py").write_text("import asyncio\n" + DB + f"\nasyncio.run({CALL.strip()})\n")
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
    assert shutil.which(command[0]), f"{command[0]} is a dev dependency"
    source = tmp_path / "db.py"
    source.write_text(DB + CALL)
    _generator().generate_stub_file(source, mode="incremental,create")
    (tmp_path / "use.py").write_text(USE)
    result = subprocess.run(command, capture_output=True, text=True, cwd=tmp_path)
    assert result.returncode == 0, result.stdout + result.stderr

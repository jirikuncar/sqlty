"""Stub generation with ``untyped = "sentinel"``: SQL[UNTYPED] instead of Any/str."""

import os
import subprocess
import sys
from pathlib import Path

import pytest

from sqlty._generator import TypeStubGenerator
from sqlty._stub_parser import PyiToIRParser

SOURCE = """from sqlty import sqlty


@sqlty
def sql(query: str) -> str:
    return query
"""

CALLS = """
users = sql("SELECT id::INTEGER FROM users")
pairs = sql("VALUES (1, 2)")
purge = sql("DELETE FROM users")
"""

SENTINEL_STUB = """# mypy: disable-error-code="overload-overlap"
# pyright: reportOverlappingOverload=false
from typing import Literal, overload, Any
from sqlty import SQL, UNTYPED


@overload
def sql(query: Literal['DELETE FROM users']) -> SQL[Any]:
    ...

@overload
def sql(query: Literal['SELECT id::INTEGER FROM users']) -> SQL[tuple[int]]:
    ...

@overload
def sql(query: Literal['VALUES (1, 2)']) -> SQL[UNTYPED]:
    ...

@overload
def sql(query: str) -> SQL[UNTYPED]:
    ...
"""


def _generator(untyped: str = "sentinel", **config: object) -> TypeStubGenerator:
    return TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query", **config}],
        untyped=untyped,
    )


def _queries(tmp_path: Path, calls: str = CALLS) -> Path:
    path = tmp_path / "queries.py"
    path.write_text(SOURCE + calls)
    return path


def test_sentinel_stub(tmp_path: Path) -> None:
    stub = _generator().generate_stub_file(_queries(tmp_path), mode="incremental,create")
    assert stub.read_text() == SENTINEL_STUB


def test_skip_is_default(tmp_path: Path) -> None:
    stub = TypeStubGenerator(
        function_configs=[{"import_path": "queries.sql", "query_param": "query"}]
    ).generate_stub_file(_queries(tmp_path), mode="incremental,create")
    content = stub.read_text()
    assert "UNTYPED" not in content
    assert "def sql(query: Literal['VALUES (1, 2)']) -> SQL[Any]:" in content
    assert content.endswith("def sql(query: str) -> str:\n    ...\n")


def test_uninferable_query_is_not_warned(tmp_path: Path, caplog: pytest.LogCaptureFixture) -> None:
    _generator().generate_stub_file(_queries(tmp_path), mode="incremental,create")
    warned = [r.getMessage() for r in caplog.records if "Unable to infer" in r.getMessage()]
    assert len(warned) == 1 and "DELETE FROM users" in warned[0]


def test_custom_template(tmp_path: Path) -> None:
    stub = _generator(
        return_type_template="Cursor[{sqlty}]", imports=["from psycopg import Cursor"]
    ).generate_stub_file(_queries(tmp_path), mode="incremental,create")
    content = stub.read_text()
    assert "from psycopg import Cursor\nfrom sqlty import UNTYPED\n" in content
    assert "def sql(query: Literal['VALUES (1, 2)']) -> Cursor[UNTYPED]:" in content
    assert content.endswith("def sql(query: str) -> Cursor[UNTYPED]:\n    ...\n")


def test_no_calls_stub(tmp_path: Path) -> None:
    stub = _generator().generate_stub_file(_queries(tmp_path, ""), mode="incremental,create")
    assert stub.read_text() == (
        '# mypy: disable-error-code="overload-overlap"\n'
        "# pyright: reportOverlappingOverload=false\n"
        "from typing import Literal, overload\n"
        "from sqlty import SQL, UNTYPED\n\n\n"
        "@overload\n"
        "def sql(query: str) -> SQL[UNTYPED]:\n"
        "    ...\n"
    )


def test_idempotent(tmp_path: Path) -> None:
    queries = _queries(tmp_path)
    stub = _generator().generate_stub_file(queries, mode="incremental,create")
    mtime = os.path.getmtime(stub)
    _generator().generate_stub_file(tmp_path, mode="incremental,prune,create")
    assert stub.read_text() == SENTINEL_STUB
    assert os.path.getmtime(stub) == mtime


def test_parser_round_trip(tmp_path: Path) -> None:
    stub = _generator().generate_stub_file(_queries(tmp_path), mode="incremental,create")
    registry = PyiToIRParser().parse_file(stub).registries["sql"]
    assert registry.fallback_return_type == "SQL[UNTYPED]"
    assert {o.return_type for o in registry.overloads} >= {"SQL[UNTYPED]"}


def test_incremental_adds_untyped_overload(tmp_path: Path) -> None:
    queries = _queries(tmp_path, 'users = sql("SELECT id::INTEGER FROM users")\n')
    stub = _generator().generate_stub_file(queries, mode="incremental,create")
    queries.write_text(SOURCE + CALLS)
    _generator().generate_stub_file(queries, mode="incremental")
    assert stub.read_text() == SENTINEL_STUB


@pytest.mark.parametrize(("before", "after"), [("skip", "sentinel"), ("sentinel", "skip")])
def test_switching_mode_updates_fallback(tmp_path: Path, before: str, after: str) -> None:
    queries = _queries(tmp_path, 'users = sql("SELECT id::INTEGER FROM users")\n')
    stub = _generator(before).generate_stub_file(queries, mode="incremental,create")
    _generator(after).generate_stub_file(tmp_path, mode="incremental,prune,create")
    fresh = tmp_path / "fresh"
    fresh.mkdir()
    expected = _generator(after).generate_stub_file(
        _queries(fresh, 'users = sql("SELECT id::INTEGER FROM users")\n'), mode="create"
    )
    assert stub.read_text() == expected.read_text()


def test_check(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    queries = _queries(tmp_path)
    _generator().generate_stub_file(queries, mode="incremental,create")
    capsys.readouterr()

    assert _generator().check(queries) == 0
    assert _generator("skip").check(queries) == 1
    assert "<fallback> → SQL[UNTYPED]" in capsys.readouterr().out


def test_check_detects_missing_sentinel(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    queries = _queries(tmp_path)
    _generator("skip").generate_stub_file(queries, mode="incremental,create")
    capsys.readouterr()

    assert _generator("skip").check(queries) == 0
    assert _generator().check(queries) == 1
    assert "+ 1 overload(s) added" in capsys.readouterr().out


def test_auto_generate(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nuntyped = "sentinel"\n\n'
        '[[tool.sqlty.functions]]\nimport_path = "__main__.sql"\nquery_param = "query"\n'
    )
    module = _queries(tmp_path, 'pairs = sql("VALUES (1, 2)")\n')
    result = subprocess.run(
        [sys.executable, str(module)],
        capture_output=True,
        text=True,
        env={**os.environ, "SQLTY_AUTO_GENERATE": "1"},
    )
    assert result.returncode == 0, result.stderr
    content = module.with_suffix(".pyi").read_text()
    assert "from sqlty import SQL, UNTYPED" in content
    assert "def sql(query: Literal['VALUES (1, 2)']) -> SQL[UNTYPED]:" in content
    assert "def sql(query: str) -> SQL[UNTYPED]:" in content


def test_stubtest(tmp_path: Path) -> None:
    queries = _queries(tmp_path, "")
    (tmp_path / "app.py").write_text("from queries import sql\n" + CALLS)
    _generator().generate_stub_file(tmp_path, mode="incremental,create")
    assert "SQL[UNTYPED]" in queries.with_suffix(".pyi").read_text()
    src = Path(__file__).parents[2] / "src"
    result = subprocess.run(
        [sys.executable, "-m", "mypy.stubtest", "queries", "--concise"],
        capture_output=True,
        text=True,
        cwd=tmp_path,
        env={**os.environ, "PYTHONPATH": f"{tmp_path}:{src}", "MYPYPATH": str(tmp_path)},
    )
    assert result.returncode == 0, result.stdout + result.stderr

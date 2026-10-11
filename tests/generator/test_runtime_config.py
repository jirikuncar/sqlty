"""Runtime stub generation honors every ``[tool.sqlty]`` and ``[[tool.sqlty.functions]]`` key."""

import datetime
import os
import subprocess
import sys
import typing
from pathlib import Path

import pytest

from sqlty._runtime_registry import type_expression

SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n"

APP = """import datetime
from sqlty import sqlty


@sqlty
def sql(query: str, return_tp: object = None) -> object:
    return query


users = sql("SELECT id, name FROM users")
"""


def _run(tmp_path: Path, pyproject: str, app: str = APP) -> subprocess.CompletedProcess[str]:
    (tmp_path / "pyproject.toml").write_text(pyproject)
    (tmp_path / "app.py").write_text(app)
    (tmp_path / "main.py").write_text("import app\n")
    return subprocess.run(
        [sys.executable, "main.py"],
        capture_output=True,
        text=True,
        cwd=tmp_path,
        env={**os.environ, "SQLTY_AUTO_GENERATE": "1"},
    )


def _stub(tmp_path: Path, pyproject: str, app: str = APP) -> str:
    result = _run(tmp_path, pyproject, app)
    assert result.returncode == 0, result.stderr
    return (tmp_path / "app.pyi").read_text()


FUNCTION = '[[tool.sqlty.functions]]\nimport_path = "app.sql"\nquery_param = "query"\n'


def test_function_schema(tmp_path: Path) -> None:
    (tmp_path / "db").mkdir()
    (tmp_path / "db" / "schema.sql").write_text(SCHEMA)
    stub = _stub(tmp_path, FUNCTION + 'schema = "db/schema.sql"\n')
    assert "-> SQL[tuple[int, str]]" in stub


@pytest.mark.parametrize("key", ["app", "app.sql"])
def test_schemas(tmp_path: Path, key: str) -> None:
    (tmp_path / "users.sql").write_text(SCHEMA)
    stub = _stub(tmp_path, f'[tool.sqlty.schemas]\n"{key}" = "users.sql"\n\n' + FUNCTION)
    assert "-> SQL[tuple[int, str]]" in stub


def test_top_level_schema(tmp_path: Path) -> None:
    (tmp_path / "schema.sql").write_text(SCHEMA)
    stub = _stub(tmp_path, '[tool.sqlty]\nschema = "schema.sql"\n\n' + FUNCTION)
    assert "-> SQL[tuple[int, str]]" in stub


def test_schema_precedence(tmp_path: Path) -> None:
    """Function ``schema`` > ``schemas`` > ``@sqlty(schema=...)`` > top-level ``schema``."""
    for name, id_type in [("function", "INTEGER"), ("mapped", "TEXT"), ("decorator", "DATE")]:
        (tmp_path / f"{name}.sql").write_text(f"CREATE TABLE users (id {id_type});\n")
    (tmp_path / "global.sql").write_text("CREATE TABLE users (id BOOLEAN);\n")
    plain = APP.replace("id, name", "id")
    decorated = plain.replace("@sqlty\n", '@sqlty(schema="decorator.sql")\n')
    top = '[tool.sqlty]\nschema = "global.sql"\n\n'
    mapped = top + '[tool.sqlty.schemas]\napp = "mapped.sql"\n\n'
    cases = [
        (mapped + FUNCTION + 'schema = "function.sql"\n', decorated, "SQL[tuple[int]]"),
        (mapped + FUNCTION, decorated, "SQL[tuple[str]]"),
        (top + FUNCTION, decorated, "SQL[tuple[datetime.date]]"),
        (top + FUNCTION, plain, "SQL[tuple[bool]]"),
    ]
    for pyproject, app, expected in cases:
        (tmp_path / "app.pyi").unlink(missing_ok=True)
        assert expected in _stub(tmp_path, pyproject, app)


def test_imports(tmp_path: Path) -> None:
    stub = _stub(tmp_path, FUNCTION + 'imports = ["from decimal import Decimal"]\n')
    assert "from decimal import Decimal" in stub


def test_format_command(tmp_path: Path) -> None:
    command = r"""format-command = "sh -c 'echo \"# formatted\"; cat' {filename}" """
    stub = _stub(tmp_path, "[tool.sqlty]\n" + command + "\n\n" + FUNCTION)
    assert stub.startswith("# formatted\n")
    assert "def sql(" in stub


def test_return_type_param(tmp_path: Path) -> None:
    app = APP + 'one = sql("SELECT 1", return_tp=int)\n'
    app += 'days = sql("SELECT now()", return_tp=list[datetime.date])\n'
    stub = _stub(tmp_path, FUNCTION + 'return_type_param = "return_tp"\n', app)
    assert (
        "def sql(query: Literal['SELECT 1'], return_tp: type[int] | None=None) -> SQL[int]:"
        in stub
    )
    assert "return_tp: type[list[datetime.date]] | None=None) -> SQL[list[datetime.date]]" in stub
    assert "def sql(query: Literal['SELECT id, name FROM users'], return_tp: object=None)" in stub
    assert "import datetime" in stub

    # A second run merges into the existing stub and keeps the full signatures
    assert _stub(tmp_path, FUNCTION + 'return_type_param = "return_tp"\n', app) == stub


def test_wrapper_forwards_arguments(tmp_path: Path) -> None:
    app = """from sqlty import sqlty


@sqlty
def fetch(conn: object, sql_text: str, *, limit: int = 10, **params: object) -> object:
    return (conn, sql_text, limit, params)


print(fetch("conn", "SELECT 1", limit=5, user_id=1))
print(fetch("conn", sql_text="SELECT 2"))
"""
    config = '[[tool.sqlty.functions]]\nimport_path = "app.fetch"\nquery_param = "sql_text"\n'
    result = _run(tmp_path, config, app)
    assert result.returncode == 0, result.stderr
    assert result.stdout.splitlines() == [
        "('conn', 'SELECT 1', 5, {'user_id': 1})",
        "('conn', 'SELECT 2', 10, {})",
    ]
    stub = (tmp_path / "app.pyi").read_text()
    assert "sql_text: Literal['SELECT 1']" in stub
    assert "sql_text: Literal['SELECT 2']" in stub


@pytest.mark.parametrize(
    ("tp", "expected"),
    [
        (int, ("int", [])),
        ("Row", ("Row", [])),
        (datetime.date, ("datetime.date", ["import datetime"])),
        (list[datetime.date], ("list[datetime.date]", ["import datetime"])),
        (int | None, ("int | None", [])),
        (typing.Literal["a"], ("typing.Literal['a']", ["import typing"])),
    ],
)
def test_type_expression(tp: object, expected: tuple[str, list[str]]) -> None:
    assert type_expression(tp) == expected

"""Runtime stub generation honors every ``[tool.sqlty]`` and ``[[tool.sqlty.functions]]`` key."""

import datetime
import doctest
import os
import subprocess
import sys
import typing
from pathlib import Path
from types import ModuleType

import pytest

from sqlty import _runtime_registry, _stub_serializer
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
    """Function ``schema`` > ``schemas["app.sql"]`` > ``@sqlty(schema=...)`` > ``schemas["app"]``
    > top-level ``schema``."""
    types = {"function": "INTEGER", "fqn": "TEXT", "decorator": "DATE", "module": "BOOLEAN"}
    for name, id_type in [*types.items(), ("global", "BYTEA")]:
        (tmp_path / f"{name}.sql").write_text(f"CREATE TABLE users (id {id_type} NOT NULL);\n")
    plain = APP.replace("id, name", "id")
    decorated = plain.replace("@sqlty\n", '@sqlty(schema="decorator.sql")\n')
    top = '[tool.sqlty]\nschema = "global.sql"\n\n'
    module = top + '[tool.sqlty.schemas]\napp = "module.sql"\n'
    both = module + '"app.sql" = "fqn.sql"\n'
    cases = [
        (both + "\n" + FUNCTION + 'schema = "function.sql"\n', decorated, "SQL[tuple[int]]"),
        (both + "\n" + FUNCTION, decorated, "SQL[tuple[str]]"),
        (module + "\n" + FUNCTION, decorated, "SQL[tuple[datetime.date]]"),
        (module + "\n" + FUNCTION, plain, "SQL[tuple[bool]]"),
        (top + FUNCTION, plain, "SQL[tuple[bytes]]"),
    ]
    for pyproject, app, expected in cases:
        (tmp_path / "app.pyi").unlink(missing_ok=True)
        assert expected in _stub(tmp_path, pyproject, app)


def test_same_function_name_in_two_modules(tmp_path: Path) -> None:
    """Configs match on ``module.function``, so ``db.sql`` and ``analytics.sql`` stay apart."""
    (tmp_path / "db.sql").write_text("CREATE TABLE t (id INTEGER NOT NULL);\n")
    (tmp_path / "analytics.sql").write_text("CREATE TABLE t (id TEXT NOT NULL);\n")
    (tmp_path / "db.py").write_text(
        "from sqlty import sqlty\n\n\n@sqlty\ndef sql(query: str) -> object:\n"
        "    return query\n\n\nsql('SELECT id FROM t')\n"
    )
    (tmp_path / "analytics.py").write_text(
        "from sqlty import sqlty\n\n\n@sqlty\ndef sql(conn: object, q: str) -> object:\n"
        "    return q\n\n\nsql(None, 'SELECT id FROM t')\n"
    )
    pyproject = (
        '[[tool.sqlty.functions]]\nimport_path = "db.sql"\nquery_param = "query"\n'
        'schema = "db.sql"\n\n'
        '[[tool.sqlty.functions]]\nimport_path = "analytics.sql"\nquery_param = "q"\n'
        'schema = "analytics.sql"\nimports = ["from decimal import Decimal"]\n'
    )
    result = _run(tmp_path, pyproject, "import db\nimport analytics\n")
    assert result.returncode == 0, result.stderr
    db = (tmp_path / "db.pyi").read_text()
    analytics = (tmp_path / "analytics.pyi").read_text()
    assert "def sql(query: Literal['SELECT id FROM t']) -> SQL[tuple[int]]" in db
    assert "Decimal" not in db
    assert "def sql(conn: object, q: Literal['SELECT id FROM t']) -> SQL[tuple[str]]" in analytics
    assert "from decimal import Decimal" in analytics


def test_query_param_default(tmp_path: Path) -> None:
    """A defaulted query parameter is recorded when omitted and never keeps its default value."""
    app = """from sqlty import sqlty


@sqlty
def kw(conn: object, q: str = "SELECT 1") -> object:
    return q


@sqlty
def ko(conn: object, *, q: str = "SELECT 1") -> object:
    return q


kw("c")
kw("c", q="SELECT 3")
ko("c")
ko("c", q="SELECT 3")
"""
    pyproject = "".join(
        f'[[tool.sqlty.functions]]\nimport_path = "app.{name}"\nquery_param = "q"\n\n'
        for name in ["kw", "ko"]
    )
    stub = _stub(tmp_path, pyproject, app)
    assert "def kw(conn: object, q: Literal['SELECT 1']=...) -> SQL[tuple[int]]:" in stub
    assert "def kw(conn: object, q: Literal['SELECT 3']) -> SQL[tuple[int]]:" in stub
    assert "def ko(conn: object, *, q: Literal['SELECT 1']=...) -> SQL[tuple[int]]:" in stub
    assert "def ko(conn: object, *, q: Literal['SELECT 3']) -> SQL[tuple[int]]:" in stub
    assert "def kw(conn: Any, q: str='SELECT 1') -> Any:" in stub


def test_format_command_failure_keeps_stub(tmp_path: Path) -> None:
    stub = _stub(tmp_path, '[tool.sqlty]\nformat-command = "false {filename}"\n\n' + FUNCTION)
    assert stub.startswith("from typing import")
    assert "def sql(" in stub


def test_other_cwd(tmp_path: Path) -> None:
    """Config and schema paths resolve from ``pyproject.toml``, not the working directory."""
    (tmp_path / "users.sql").write_text(SCHEMA)
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nschema = "users.sql"\n\n' + FUNCTION + 'schema = "users.sql"\n'
    )
    (tmp_path / "app.py").write_text(APP)
    (tmp_path / "elsewhere").mkdir()
    result = subprocess.run(
        [sys.executable, str(tmp_path / "app.py")],
        capture_output=True,
        text=True,
        cwd=tmp_path / "elsewhere",
        env={**os.environ, "SQLTY_AUTO_GENERATE": "1"},
    )
    assert result.returncode == 0, result.stderr
    assert "-> SQL[tuple[int, str]]" in (tmp_path / "app.pyi").read_text()


def test_decorator_returns_function_unchanged_without_env(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text(FUNCTION)
    (tmp_path / "app.py").write_text(
        APP + "\nimport inspect\n"
        "assert sql.__code__.co_filename == __file__ and not hasattr(sql, '__wrapped__')\n"
    )
    env = {k: v for k, v in os.environ.items() if k != "SQLTY_AUTO_GENERATE"}
    result = subprocess.run(
        [sys.executable, "app.py"], capture_output=True, text=True, cwd=tmp_path, env=env
    )
    assert result.returncode == 0, result.stderr
    assert not (tmp_path / "app.pyi").exists()


def test_async_helper_stays_coroutine_function(tmp_path: Path) -> None:
    app = """import asyncio
import inspect
from sqlty import sqlty


@sqlty
async def sql(query: str) -> object:
    return query


assert inspect.iscoroutinefunction(sql)
print(asyncio.run(sql("SELECT 1")))
"""
    result = _run(tmp_path, FUNCTION, app)
    assert result.returncode == 0, result.stderr
    assert result.stdout == "SELECT 1\n"


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
        "def sql(query: Literal['SELECT 1'], return_tp: type[int] | None=None) -> SQL[int]:" in stub
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


def _local() -> type:
    class Row:
        pass

    return Row


Local = _local()


@pytest.mark.parametrize(
    ("tp", "expected"),
    [
        (int, ("int", [])),
        ("Row", ("Row", [])),
        (datetime.date, ("datetime.date", ["import datetime"])),
        (list[datetime.date], ("list[datetime.date]", ["import datetime"])),
        (int | None, ("int | None", [])),
        (typing.Literal["a"], ("typing.Literal['a']", ["import typing"])),
        (tuple[int, ...], ("tuple[int, ...]", [])),
        (Path, ("pathlib.Path", ["import pathlib"])),
        (Local, ("Any", ["from typing import Any"])),
    ],
)
def test_type_expression(tp: object, expected: tuple[str, list[str]]) -> None:
    assert type_expression(tp) == expected


def test_type_expression_home_module() -> None:
    """The stub's own module needs no import; another ``__main__`` script is unreachable."""
    assert type_expression(Path, "pathlib") == ("Path", [])
    main = type("Row", (), {"__module__": "__main__"})
    assert type_expression(main, "app") == ("Any", ["from typing import Any"])
    assert type_expression(list[main], "__main__") == ("list[Row]", [])


@pytest.mark.parametrize("module", [_runtime_registry, _stub_serializer])
def test_doctests(module: ModuleType) -> None:
    assert doctest.testmod(module).failed == 0

"""Row type fields naming types imported from other checked modules (#160)."""

from __future__ import annotations

import ast
import contextlib
import sys

import pytest

from sqlty._cli import main
from sqlty.core import HelperIndex, check_source

SCHEMA = """\
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, status TEXT NOT NULL);
"""

IDS = """\
from typing import Literal, NewType, TypeAlias

UserId = NewType("UserId", int)
AdminId = NewType("AdminId", UserId)
type Name = str
Plain = str
Alias: TypeAlias = Plain
Status = Literal["new", "old"]
"""

ROW = """\
from typing import TypedDict
{imports}
from sqlty import SQL

class Row(TypedDict):
    id: {id}
    name: {name}

query = SQL[Row]("--sql\\nSELECT {id_col} AS id, {name_col} AS name FROM users")
"""


def row(
    imports: str, id: str = "int", name: str = "str", id_col: str = "id", name_col: str = "name"
) -> str:
    return ROW.format(imports=imports, id=id, name=name, id_col=id_col, name_col=name_col)


@pytest.fixture
def project(tmp_path, monkeypatch):
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text(SCHEMA)
    pkg = tmp_path / "pkg"
    (pkg / "sub").mkdir(parents=True)
    (pkg / "__init__.py").write_text(
        "from .ids import UserId, AdminId, Name\nfrom . import ids as ids\n"
    )
    (pkg / "sub" / "__init__.py").write_text("")
    (pkg / "ids.py").write_text(IDS)
    monkeypatch.chdir(tmp_path)
    return tmp_path


def run(project, capsys, files: dict[str, str]) -> list[tuple[str, str]]:
    for name, source in files.items():
        (project / name).write_text(source)
    with contextlib.suppress(SystemExit):
        main(["check", "."])
    return sorted(
        (line.split(":")[0].rsplit("/", 1)[-1], line.split("[")[1].split("]")[0])
        for line in capsys.readouterr().out.splitlines()
        if "[" in line
    )


def test_newtype_from_another_module(project, capsys):
    files = {
        "ok.py": row("from pkg.ids import UserId", id="UserId"),
        "bad.py": row("from pkg.ids import UserId", id="UserId", id_col="name"),
    }
    assert run(project, capsys, files) == [("bad.py", "column-type-mismatch")]


def test_import_forms(project, capsys):
    files = {
        "aliased.py": row("from pkg.ids import UserId as U", id="U", id_col="name"),
        "module_alias.py": row("import pkg.ids as ids", id="ids.UserId", id_col="name"),
        "dotted.py": row("import pkg.ids", id="pkg.ids.UserId", id_col="name"),
        "from_package.py": row("from pkg import ids", id="ids.UserId", id_col="name"),
        "reexport.py": row("from pkg import UserId", id="UserId", id_col="name"),
        "pkg/sub/relative.py": row("from ..ids import UserId", id="UserId", id_col="name"),
        "pkg/sub/sibling.py": row("from .. import ids", id="ids.UserId", id_col="name"),
        "pkg/sub/reexport.py": row("from .. import UserId", id="UserId", id_col="name"),
        "ok.py": row("from pkg import ids, UserId", id="ids.UserId", name="ids.Name"),
    }
    found = run(project, capsys, files)
    assert found == sorted(
        (n.rsplit("/", 1)[-1], "column-type-mismatch")
        for n in files
        if n not in ("ok.py", "literal.py")
    )


def test_alias_chains(project, capsys):
    imports = "from pkg.ids import AdminId, Name, Plain, Alias, Status"
    files = {
        "newtype_chain.py": row(imports, id="AdminId", id_col="name"),
        "type_stmt.py": row(imports, name="Name", name_col="id"),
        "plain_alias.py": row(imports, name="Plain", name_col="id"),
        "type_alias.py": row(imports, name="Alias", name_col="id"),
        "ok.py": row(imports, id="AdminId", name="Alias | None"),
        "literal.py": row(imports, name="Status"),
    }
    assert run(project, capsys, files) == sorted(
        (n, "column-type-mismatch") for n in files if n not in ("ok.py", "literal.py")
    )


def test_cycles_terminate(project, capsys):
    (project / "a.py").write_text("from b import B, Y\nX = Y\nA = B\n")
    (project / "b.py").write_text(
        'from typing import NewType\nfrom a import A, X\nB = NewType("B", int)\nY = X\n'
    )
    files = {"use.py": row("from a import A, X", id="A", name="X")}
    assert run(project, capsys, files) == []


def test_import_from_outside_checked_paths_stays_unknown(project, capsys):
    files = {"use.py": row("from myapp.ids import UserId", id="UserId", id_col="name")}
    assert run(project, capsys, files) == []


def test_checked_modules_are_never_imported(project, capsys):
    marker = project / "imported.txt"
    (project / "pkg" / "ids.py").write_text(f"open({str(marker)!r}, 'w')\n" + IDS)
    sys.path.insert(0, str(project))
    try:
        files = {"use.py": row("from pkg.ids import UserId", id="UserId", id_col="name")}
        assert run(project, capsys, files) == [("use.py", "column-type-mismatch")]
    finally:
        sys.path.remove(str(project))
    assert not marker.exists()


def test_modules_are_parsed_once_and_only_when_referenced(monkeypatch):
    index = HelperIndex()
    index.add("pkg.ids", IDS)
    index.add("pkg.other", "import not_parsed_unless_referenced\nX = int\n")
    parses = []
    real = ast.parse
    monkeypatch.setattr(ast, "parse", lambda s, *a, **k: parses.append(s) or real(s, *a, **k))
    source = row("from pkg.ids import UserId", id="UserId", id_col="name")
    for _ in range(3):
        diagnostics = check_source(source, schema_sql=SCHEMA, helpers=index, module="app")
    assert [d["rule"] for d in diagnostics] == ["column-type-mismatch"]
    assert parses.count(IDS) == 1
    assert not any("not_parsed_unless_referenced" in p for p in parses)


def test_without_index_unresolved_names_are_not_reported():
    source = row("from pkg.ids import UserId", id="UserId", id_col="name")
    assert check_source(source) == []


def test_cross_module_resolution_parses_each_module_once(project, capsys, monkeypatch):
    import ast
    from collections import Counter

    parses: Counter[str] = Counter()
    real = ast.parse

    def counting(source, *args, **kwargs):
        if isinstance(source, str) and kwargs.get("mode") is None and "mode" not in args[:2]:
            parses[source] += 1
        return real(source, *args, **kwargs)

    monkeypatch.setattr(ast, "parse", counting)
    files = {"bad.py": row("from pkg.ids import UserId", id="UserId", id_col="name")}
    assert run(project, capsys, files) == [("bad.py", "column-type-mismatch")]
    assert parses[IDS] == 1
    assert max(parses.values()) == 1

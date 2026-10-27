"""Annotated query helpers across modules (the mdtest runner has one module per test)."""

from __future__ import annotations

import ast

import pytest

from sqlty._cli import main
from sqlty.core import HelperIndex, check_source

SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n"

DB = """\
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...
"""


def rules(output: str) -> list[tuple[str, str]]:
    return [
        (line.split(":")[0].rsplit("/", 1)[-1], line.split("[")[1].split("]")[0])
        for line in output.splitlines()
        if "[" in line
    ]


@pytest.fixture
def project(tmp_path, monkeypatch):
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text(SCHEMA)
    pkg = tmp_path / "pkg"
    (pkg / "sub").mkdir(parents=True)
    (pkg / "__init__.py").write_text("")
    (pkg / "sub" / "__init__.py").write_text("")
    (pkg / "db.py").write_text(DB)
    monkeypatch.chdir(tmp_path)
    return tmp_path


def test_helpers_of_other_modules(project, capsys):
    """`from`, aliased, module and relative imports all reach the helper."""
    bad = 'fetch_all("SELECT name FROM users", int)\n'
    sources = {
        "from_import.py": "from pkg.db import fetch_all\n" + bad,
        "aliased.py": "from pkg.db import fetch_all as fa\n" + bad.replace("fetch_all", "fa"),
        "module_alias.py": "import pkg.db as m\n" + bad.replace("fetch_all", "m.fetch_all"),
        "dotted.py": "import pkg.db\n" + bad.replace("fetch_all", "pkg.db.fetch_all"),
        "from_package.py": "from pkg import db\n" + bad.replace("fetch_all", "db.fetch_all"),
        "pkg/sub/relative.py": "from ..db import fetch_all\n" + bad,
        "pkg/sub/sibling.py": "from .. import db\n" + bad.replace("fetch_all", "db.fetch_all"),
        "clean.py": 'from pkg.db import fetch_all\nfetch_all("SELECT name FROM users", str)\n',
        "unrelated.py": 'from other import fetch_all\nfetch_all("SELECT name FROM users", int)\n',
    }
    for name, source in sources.items():
        (project / name).write_text(source)

    with pytest.raises(SystemExit):
        main(["check", "."])
    found = sorted(rules(capsys.readouterr().out))
    flagged = sorted(n.rsplit("/", 1)[-1] for n in sources if n not in ("clean.py", "unrelated.py"))
    assert found == sorted((n, "column-type-mismatch") for n in flagged)


def test_checking_a_subdirectory(project, capsys):
    """A checked directory that is itself the package resolves `pkg.` imports."""
    (project / "pkg" / "use.py").write_text(
        'from pkg.db import fetch_all\nfetch_all("SELECT name FROM users", int)\n'
    )
    with pytest.raises(SystemExit):
        main(["check", "pkg"])
    assert rules(capsys.readouterr().out) == [("use.py", "column-type-mismatch")]


def test_rewrite_and_strip_leave_calls_alone(project):
    source = 'from pkg.db import fetch_all\nfetch_all("SELECT name FROM users", str)\n'
    (project / "use.py").write_text(source)
    main(["rewrite", "use.py"])
    assert (project / "use.py").read_text() == source
    main(["strip", "use.py"])
    assert (project / "use.py").read_text() == source


def test_check_source_with_index():
    index = HelperIndex()
    assert index.add("db", DB)
    assert not index.add("other", "x = 1\n")
    use = 'from db import fetch_all\nids = fetch_all("SELECT 1::TEXT", int)\n'
    assert [d["rule"] for d in check_source(use, helpers=index)] == ["column-type-mismatch"]
    assert check_source(use) == []
    assert check_source(use.replace("db", "elsewhere"), helpers=index) == []


def test_unbindable_calls_are_skipped():
    """Starred arguments and missing row parameters leave nothing to check."""
    source = DB + (
        "args = ()\n"
        "fetch_all(*args, int)\n"
        'fetch_all("SELECT 1::TEXT")\n'
        'fetch_all(f"SELECT {1}::TEXT", int)\n'
    )
    assert check_source(source) == []


def test_ignore_comment_suppresses_helper_diagnostics() -> None:
    call = 'fetch_all("SELECT name FROM users", int)'
    assert check_source(DB + call + "\n", schema_sql=SCHEMA)
    assert (
        check_source(DB + call + "  # sqlty: ignore[column-type-mismatch]\n", schema_sql=SCHEMA)
        == []
    )


METHODS = """\
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def fetch_all[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

    @classmethod
    def fetch_one[T](cls, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...
"""
BAD = '("SELECT name FROM users", int)\n'


def test_method_receivers(project, capsys):
    """Class, instance, annotation, `self` and `self.attr` receivers all bind the arguments."""
    (project / "pkg" / "methods.py").write_text(METHODS)
    import_db = "from pkg.methods import Db\n"
    sources = {
        "assigned.py": import_db + "db = Db()\ndb.fetch_all" + BAD,
        "annotated.py": import_db + "def f(db: Db):\n    db.fetch_all" + BAD,
        "classmethod.py": import_db + "Db.fetch_one" + BAD,
        "dotted.py": "import pkg.methods as m\ndb: m.Db = m.Db()\ndb.fetch_all" + BAD,
        "local.py": METHODS + "def f():\n    db = Db()\n    db.fetch_all" + BAD,
        "inside.py": METHODS + "    def run(self):\n        self.fetch_all" + BAD,
        "attribute.py": import_db
        + "class Repo:\n    def __init__(self):\n        self.db = Db()\n"
        + "    def run(self):\n        self.db.fetch_all"
        + BAD,
        "unbound.py": import_db + "Db.fetch_all" + BAD,
        "reassigned.py": import_db + "db = Db()\ndb = other()\ndb.fetch_all" + BAD,
        "unrelated.py": "class Other:\n    def fetch_all(self, q, t): ...\n"
        + "o = Other()\no.fetch_all"
        + BAD,
    }
    for name, source in sources.items():
        (project / name).write_text(source)
    with pytest.raises(SystemExit):
        main(["check", "."])
    found = [n for n, rule in rules(capsys.readouterr().out) if rule == "column-type-mismatch"]
    assert sorted(found) == [
        "annotated.py",
        "assigned.py",
        "attribute.py",
        "classmethod.py",
        "dotted.py",
        "inside.py",
        "local.py",
    ]


def test_package_reexports(project, capsys):
    (project / "pkg" / "__init__.py").write_text("from .db import fetch_all\n")
    (project / "use.py").write_text("from pkg import fetch_all\nfetch_all" + BAD)
    (project / "via_module.py").write_text("import pkg\npkg.fetch_all" + BAD)
    with pytest.raises(SystemExit):
        main(["check", "."])
    assert sorted(rules(capsys.readouterr().out)) == [
        ("use.py", "column-type-mismatch"),
        ("via_module.py", "column-type-mismatch"),
    ]


def test_configured_functions(project, capsys):
    """`[[tool.sqlty.functions]]` registers helpers without annotations."""
    entries = "".join(
        f'[[tool.sqlty.functions]]\nimport-path = "{path}"\nquery-param = "sql"\n'
        'return-type-param = "form"\n'
        for path in ("pkg.plain.run", "pkg.plain.Db.get", "lib.run")
    )
    (project / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n' + entries)
    (project / "pkg" / "plain.py").write_text(
        "def run(sql, form): ...\nclass Db:\n    def get(self, sql, form): ...\n"
    )
    (project / "positional.py").write_text("from pkg.plain import run\nrun" + BAD)
    (project / "method.py").write_text(
        "from pkg.plain import Db\nDb().get(form=int, sql='SELECT name FROM users')\n"
    )
    (project / "instance.py").write_text("from pkg.plain import Db\ndb = Db()\ndb.get" + BAD)
    (project / "library.py").write_text(
        "from lib import run\nrun(sql='SELECT name FROM users', form=int)\n"
    )
    (project / "library_positional.py").write_text("from lib import run\nrun" + BAD)
    with pytest.raises(SystemExit):
        main(["check", "."])
    assert sorted(rules(capsys.readouterr().out)) == [
        ("instance.py", "column-type-mismatch"),
        ("library.py", "column-type-mismatch"),
        ("positional.py", "column-type-mismatch"),
    ]


def test_register_reads_unindexed_modules_from_roots(tmp_path):
    """`roots` give the positional parameters of a module not indexed; an indexed one wins."""
    (tmp_path / "pkg" / "sub").mkdir(parents=True)
    (tmp_path / "pkg" / "h.py").write_text("def run(sql, form): ...\n")
    (tmp_path / "pkg" / "sub" / "__init__.py").write_text("def run(conn, sql, form): ...\n")
    (tmp_path / "pkg" / "idx.py").write_text("def run(sql, form): ...\n")
    index = HelperIndex()
    index.add("pkg.idx", "def run(form, sql): ...\n")
    index.add("pkg", "VERSION = 1\n")
    for path in ("pkg.h.run", "pkg.sub.run", "pkg.idx.run"):
        index.register(path, "sql", "form", roots=[tmp_path / "missing", tmp_path])
    assert index.modules["pkg.h"]["run"].positional == ("sql", "form")
    assert index.modules["pkg.sub"]["run"].positional == ("conn", "sql", "form")
    assert index.modules["pkg.idx"]["run"].positional == ("form", "sql")


COMPOSED_DB = """\
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...
"""


def test_compositions_use_fragments_of_other_modules(project, capsys, monkeypatch):
    """Names imported from checked modules resolve to their one literal binding."""
    pytest.importorskip("psycopg")
    parses: list[str] = []
    real = ast.parse
    monkeypatch.setattr(ast, "parse", lambda s, *a, **k: parses.append(s) or real(s, *a, **k))
    (project / "pkg" / "db.py").write_text(COMPOSED_DB)
    (project / "pkg" / "columns.py").write_text(
        'from psycopg import sql\nUSER_COLUMNS = sql.SQL("id, name")\n'
        "LOOKED_UP = USER_COLUMNS\n"
        "DYNAMIC = sql.Identifier(input())\nTWICE = sql.SQL('id')\nTWICE = sql.SQL('name')\n"
    )
    (project / "pkg" / "__init__.py").write_text("from .columns import USER_COLUMNS as EXPORTED\n")
    head = "from psycopg import sql\nfrom pkg.db import fetch_all\n"
    query = 'fetch_all(sql.SQL("SELECT {} FROM users").format(%s), tuple[int, int])\n'
    sources = {
        "imported.py": head + "from pkg.columns import USER_COLUMNS\n" + query % "USER_COLUMNS",
        "relative.py": head + "from pkg import columns as c\n" + query % "c.USER_COLUMNS",
        "module.py": head + "import pkg.columns\n" + query % "pkg.columns.LOOKED_UP",
        "reexported.py": head + "from pkg import EXPORTED\n" + query % "EXPORTED",
        "dynamic.py": head
        + "from pkg.columns import DYNAMIC, TWICE\n"
        + query % "DYNAMIC"
        + query % "TWICE",
        "shadowed.py": head
        + "from pkg.columns import USER_COLUMNS\ndef f(USER_COLUMNS):\n    "
        + query % "USER_COLUMNS",
        "unknown.py": head + "from elsewhere import USER_COLUMNS\n" + query % "USER_COLUMNS",
    }
    for name, source in sources.items():
        (project / name).write_text(source)
    with pytest.raises(SystemExit):
        main(["check", "."])
    assert sorted(rules(capsys.readouterr().out)) == [
        ("dynamic.py", "unchecked-query"),
        ("dynamic.py", "unchecked-query"),
        ("imported.py", "column-type-mismatch"),
        ("module.py", "column-type-mismatch"),
        ("reexported.py", "column-type-mismatch"),
        ("relative.py", "column-type-mismatch"),
        ("shadowed.py", "unchecked-query"),
        ("unknown.py", "unchecked-query"),
    ]
    columns = (project / "pkg" / "columns.py").read_text()
    assert parses.count(columns) == 1

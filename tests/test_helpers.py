"""Annotated query helpers across modules (the mdtest runner has one module per test)."""

from __future__ import annotations

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
        f'[[tool.sqlty.functions]]\nimport_path = "{path}"\nquery_param = "sql"\n'
        'return_type_param = "form"\n'
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

"""Annotated query helpers across modules (the mdtest runner has one module per test)."""

from __future__ import annotations

import ast

import pytest

from sqlty._cli import main
from sqlty.core import HelperIndex, Settings, check_source

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
    assert [d["rule"] for d in check_source(use, Settings(helpers=index))] == [
        "column-type-mismatch"
    ]
    assert check_source(use) == []
    assert check_source(use.replace("db", "elsewhere"), Settings(helpers=index)) == []


def test_statistics_count_helper_queries(project, capsys):
    (project / "use.py").write_text(
        "from pkg.db import fetch_all\n"
        'fetch_all("SELECT id FROM users", int)\n'
        'fetch_all("SELECT nope(", int)\n'
        'fetch_all("DELETE FROM users", int)\n'
        "fetch_all(QUERY, int)\n"
        'q = f"--sql\\nSELECT {1}"\n'
    )
    main(["check", "--statistics", "."])
    assert capsys.readouterr().out.splitlines()[-1] == (
        "Queries: 1 typed, 0 partially typed, 1 unparsed, 2 skipped"
        " (fstring-skipped: 1, dynamic-query: 1)"
    )
    (project / "pyproject.toml").write_text("[tool.sqlty]\n")
    main(["check", "--statistics", "."])
    assert "1 partially typed" in capsys.readouterr().out.splitlines()[-1]


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
    assert check_source(DB + call + "\n", Settings(schema_sql=SCHEMA))
    assert (
        check_source(
            DB + call + "  # sqlty: ignore[column-type-mismatch]\n", Settings(schema_sql=SCHEMA)
        )
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
        ("method.py", "column-type-mismatch"),
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


def test_outside_reads_the_class_of_a_receiver_from_roots(tmp_path):
    """`outside` roots give the class of a receiver: `lib-stubs`, a `.pyi`, then the `.py`."""
    for name in ("lib", "lib-stubs", "plain"):
        (tmp_path / name).mkdir()
    (tmp_path / "lib" / "__init__.py").write_text("class Db:\n    def get(self, sql): ...\n")
    (tmp_path / "lib-stubs" / "__init__.pyi").write_text(
        "class Db:\n    def get(self, sql: str) -> None: ...\nclass Site:\n    db: Db\n"
    )
    (tmp_path / "plain" / "__init__.py").write_text(
        "from lib import Db\nclass Site:\n    db: Db\n    def other(self): ...\n"
    )
    (tmp_path / "big.py").write_text("class Site:\n    db = None\n" + "#" * (300 << 10))
    use = "from {} import Site\ndef run(site: Site) -> None:\n    site.db.get('SELECT 1 +')\n"
    index = HelperIndex()
    index.register("lib.Db.get", "sql", roots=[tmp_path])
    for module, found in (("lib", 1), ("plain", 1), ("big", 0), ("missing", 0)):
        assert len(check_source(use.format(module), Settings(helpers=index))) == found, module
    # Without roots the library is not read: `Site` is known by its path only.
    unread = HelperIndex()
    unread.register("lib.Db.get", "sql")
    assert check_source(use.format("lib"), Settings(helpers=unread)) == []
    # `outside` gives the roots to what is registered after it.
    read = HelperIndex()
    read.outside([tmp_path])
    read.register("lib.Db.get", "sql")
    assert len(check_source(use.format("plain"), Settings(helpers=read))) == 1


def test_register_reads_past_a_stub_that_lacks_the_helper(tmp_path):
    """A stub describing part of a library leaves the rest to the library."""
    for name in ("part", "part-stubs", "bad", "typings"):
        (tmp_path / name).mkdir()
    (tmp_path / "half.py").write_text("def fetch(sql, form): ...\ndef both(a, sql): ...\n")
    (tmp_path / "half.pyi").write_text("def both(sql: str, a: int) -> None: ...\n")
    (tmp_path / "part" / "__init__.py").write_text("def fetch(sql, form): ...\n")
    (tmp_path / "part-stubs" / "__init__.pyi").write_text("VERSION: str\n")
    (tmp_path / "bad" / "__init__.py").write_text("def fetch(sql, form): ...\n")
    (tmp_path / "bad" / "__init__.pyi").write_text("def fetch(:\n")
    index = HelperIndex()
    index.add_stub("given", "VERSION: str\n")
    index.add("given", "def fetch(sql, form): ...\n")
    for module in ("half", "part", "bad", "given"):
        index.register(f"{module}.fetch", "sql", "form", roots=[tmp_path])
        assert index.modules[module]["fetch"].positional == ("sql", "form"), module
    # What the stub does define is read from the stub.
    index.register("half.both", "sql", roots=[tmp_path])
    assert index.modules["half"]["both"].positional == ("sql", "a")


def test_register_stops_at_a_stub_that_declares_the_class(tmp_path):
    """A stub's class without the method ends the search: the library's parameters may differ."""
    (tmp_path / "lib.py").write_text("class Engine:\n    def fetch(self, label, sql): ...\n")
    (tmp_path / "lib.pyi").write_text(
        "class Base:\n    def fetch(self, sql: str, label: str = ...) -> None: ...\n"
        "class Engine(Base): ...\n"
    )
    index = HelperIndex()
    index.register("lib.Engine.fetch", "sql", roots=[tmp_path])
    found = index.modules["lib"]["Engine.fetch"]
    assert found.positional == () and found.keywords == {"sql"}


def test_register_follows_reexports(tmp_path):
    """A name a package re-exports is read from the module that defines it."""
    (tmp_path / "lib").mkdir()
    (tmp_path / "lib" / "__init__.py").write_text("from .core import Db, run as fetch\n")
    (tmp_path / "lib" / "core.py").write_text(
        "class Db:\n    async def execute(self, sql, parameters=None): ...\n"
        "def run(conn, sql, form): ...\n"
    )
    index = HelperIndex()
    for path in ("lib.Db.execute", "lib.fetch"):
        index.register(path, "sql", "form", roots=[tmp_path])
    assert index.modules["lib"]["Db.execute"].positional == ("sql", "parameters")
    assert index.modules["lib"]["Db.execute"].params == "parameters"
    assert index.modules["lib"]["fetch"].positional == ("conn", "sql", "form")


def test_register_reads_reexports_per_project(tmp_path):
    """Two projects re-exporting a helper from different modules each get their own."""
    for name, target, signature in (("a", "core", "conn, sql"), ("b", "other", "sql, conn")):
        (tmp_path / name / "pkg").mkdir(parents=True)
        (tmp_path / name / "pkg" / "__init__.py").write_text(f"from .{target} import run\n")
        (tmp_path / name / "pkg" / f"{target}.py").write_text(f"def run({signature}): ...\n")
    base = HelperIndex()
    found = {}
    for name in "ab":
        fork = base.fork()
        fork.register("pkg.run", "sql", roots=[tmp_path / name])
        found[name] = fork.modules["pkg"]["run"].positional
    assert found == {"a": ("conn", "sql"), "b": ("sql", "conn")}


def test_register_stops_on_reexport_cycles(tmp_path):
    (tmp_path / "a").mkdir()
    (tmp_path / "a" / "__init__.py").write_text("from .b import X\n")
    (tmp_path / "a" / "b.py").write_text("from a import X\n")
    index = HelperIndex()
    index.register("a.X.run", "sql", roots=[tmp_path])
    assert index.modules["a"]["X.run"].positional == ()


COMPOSED_DB = """\
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...
"""


def test_query_variants_with_preview(project, capsys):
    """`preview` checks each variant, counts each in `--statistics`, and never runs the code."""
    (project / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\npreview = true\n')
    marker = project / "ran"
    (project / "pkg" / "names.py").write_text("TABLE = 'users'\nWHERE = ' WHERE TRUE'\n")
    (project / "app.py").write_text(
        "from pkg import names\n"
        "from pkg.db import fetch_all\n"
        "from pkg.names import WHERE\n"
        f"open({str(marker)!r}, 'w')\n"
        "def column(flag: bool):\n"
        f"    open({str(marker)!r}, 'w')\n"
        "    return 'id' if flag else 'name'\n"
        "def find(flag: bool):\n"
        "    return fetch_all('SELECT ' + column(flag) + ' FROM ' + names.TABLE + WHERE, tuple[int])\n"
    )

    with pytest.raises(SystemExit):
        main(["check", "--statistics", "."])

    out = capsys.readouterr().out
    assert "column-type-mismatch" in out and "(variant: line 7)" in out
    assert out.splitlines()[-1].startswith("Queries: 2 typed, 0 partially typed")
    assert not marker.exists()


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
    assert columns not in parses

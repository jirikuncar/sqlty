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

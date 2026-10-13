"""Each source file is parsed once and tokenized only when it has a ``sqlty:`` comment."""

from __future__ import annotations

import ast
import collections
import tokenize

import pytest

from sqlty._cli import main
from sqlty._source import ParsedSource
from sqlty.core import check_source

SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n"

HELPER = """\
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...
"""

USER_TYPE = """\
from typing import TypedDict
from sqlty import SQL
from pkg.db import fetch_all

class User(TypedDict):
    id: int
    name: str

rows = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
other = fetch_all("SELECT name FROM users", str)
bad = fetch_all("SELECT name FROM users", int)  # sqlty: ignore[column-type-mismatch]
"""

PLAIN = "import os\n\n\ndef f(x):\n    return os.path.join(x, 'a')  # an ordinary comment\n"


@pytest.fixture
def parses(monkeypatch: pytest.MonkeyPatch) -> collections.Counter[str]:
    """Module-source ``ast.parse`` calls by source text (quoted annotations are not counted)."""
    counter: collections.Counter[str] = collections.Counter()
    real = ast.parse

    def counting(source, *args, **kwargs):
        if not (args or kwargs.get("mode", "exec") != "exec"):
            counter[source] += 1
        return real(source, *args, **kwargs)

    monkeypatch.setattr(ast, "parse", counting)
    return counter


@pytest.fixture
def tokenized(monkeypatch: pytest.MonkeyPatch) -> list[object]:
    calls: list[object] = []
    real = tokenize.generate_tokens

    def counting(readline):
        calls.append(readline)
        return real(readline)

    monkeypatch.setattr(tokenize, "generate_tokens", counting)
    return calls


def test_check_source_parses_once(parses):
    source = HELPER + USER_TYPE.replace("from pkg.db import fetch_all\n", "")
    diagnostics = check_source(source, schema_sql=SCHEMA)
    assert [d["rule"] for d in diagnostics] == []
    assert parses[source] == 1


def test_cli_parses_each_file_once(tmp_path, monkeypatch, parses, tokenized, capsys):
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text(SCHEMA)
    pkg = tmp_path / "pkg"
    pkg.mkdir()
    files = {"__init__.py": "", "db.py": HELPER, "app.py": USER_TYPE, "plain.py": PLAIN}
    for name, text in files.items():
        (pkg / name).write_text(text)
    monkeypatch.chdir(tmp_path)

    main(["check", "."])
    assert capsys.readouterr().out == ""

    assert parses[HELPER] == 1
    assert parses[USER_TYPE] == 1
    assert parses[PLAIN] == 0
    assert max(parses.values()) <= 1
    assert len(tokenized) == 1


def test_marker_free_file_is_neither_parsed_nor_tokenized(parses, tokenized):
    assert check_source(PLAIN) == []
    assert not parses
    assert not tokenized


def test_wrapper_without_row_type_is_not_parsed(parses):
    source = 'import os\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
    assert check_source(source) == []
    assert not parses


def test_parsed_source_is_reused(parses):
    parsed = ParsedSource(HELPER + USER_TYPE)
    check_source(parsed, schema_sql=SCHEMA)
    check_source(parsed, schema_sql=SCHEMA)
    assert parses[parsed.text] == 1


def test_parsed_source_facts():
    parsed = ParsedSource("import typing as t\nclass A:\n    class B: ...\nf(g(1))\n")
    assert parsed.typing_aliases == {"t": "typing"}
    assert [c.name for c in parsed.classes] == ["A", "B"]
    assert len(parsed.calls) == 2
    assert ParsedSource("def f(:").tree is None
    assert ParsedSource("x = 1  # sqlty: ignore[a, b]\n").ignores[1].rules == ("a", "b")
    assert ParsedSource("x = 1 # not it\n").ignores == {}

"""No source file is parsed with ``ast``, and one is tokenized only when it does not parse."""

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


def test_check_source_does_not_parse(parses):
    source = HELPER + USER_TYPE.replace("from pkg.db import fetch_all\n", "")
    diagnostics = check_source(source, schema_sql=SCHEMA)
    assert [d["rule"] for d in diagnostics] == []
    assert not parses


def test_cli_parses_no_file(tmp_path, monkeypatch, parses, tokenized, capsys):
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

    assert not parses.keys() & set(files.values())
    assert not tokenized


def test_marker_free_file_is_neither_parsed_nor_tokenized(parses, tokenized):
    assert check_source(PLAIN) == []
    assert not parses
    assert not tokenized


def test_dashes_in_wide_comments_are_skipped_quickly(parses, tokenized):
    line = (
        "        #     "
        + " ".join(["-" * 29, "x:", " " * 20, "8.8MB", " " * 20, "# s.memory"])
        + "\n"
    )
    assert check_source(line * 40) == []
    assert not parses
    assert not tokenized


def test_wrapper_without_row_type_is_not_parsed(parses):
    source = 'import os\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
    assert check_source(source) == []
    assert not parses


def test_parsed_source_is_reused(parses):
    parsed = ParsedSource(HELPER + USER_TYPE)
    assert check_source(parsed, schema_sql=SCHEMA) == check_source(parsed, schema_sql=SCHEMA)
    assert not parses


def test_parsed_source_facts():
    parsed = ParsedSource("import typing as t\nq = SQL[t.Any](x)\nf('é', g)\n")
    assert parsed.any_calls() == [23]
    assert parsed.position(parsed.offset(3, 8)) == (3, 8)
    assert ParsedSource("x = 1  # sqlty: ignore[a, b]\n").ignores[1].rules == ("a", "b")
    assert ParsedSource("x = 1 # not it\n").ignores == {}
    assert ParsedSource("def f(:  # sqlty: ignore\n").ignores[1].rules is None


def test_what_rust_does_not_model_is_reported_unchecked():
    helper = (
        "from typing import Annotated, LiteralString\nfrom psycopg import sql\n"
        "from sqlty import SQL\n"
        "def run[T](q: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...\n"
    )
    # `str.isdigit` digits that `int` may not read.
    source = helper + "run(sql.SQL('SELECT {٣}').format(1, 2, 3, 4), int)\n"
    assert [(d["rule"], d["line"]) for d in check_source(source)] == [("unchecked-query", 5)]


def test_a_source_that_does_not_parse_is_read_by_its_strings(parses, tokenized):
    source = "q = '--sql\\nSELECT 1 FROM'  # sqlty: ignore[stale-rewrite]\ndef f(:\n"
    assert [d["rule"] for d in check_source(source)] == ["unparsed-query", "unused-ignore"]
    assert not parses
    assert len(tokenized) == 1

"""Edges of the ``sqlty._core`` bindings no other test reaches."""

import sys
from collections.abc import Callable

import pytest

from sqlty import _core

VERSION = sys.version_info[:2]
UNPARSABLE = "x = (\n"
QUERY = 'q = "--sql\\nSELECT 1"\n'


def test_schema_with_a_database_without_one() -> None:
    schema = _core.load_schema("CREATE TABLE t (a INT);", databases=[("other", None)])
    assert (schema.dialect, schema.absent) == ("postgres", False)


def test_static_method_helper() -> None:
    source = (
        "from typing import Annotated\n"
        "from sqlty import SQL\n"
        "class C:\n"
        "    @staticmethod\n"
        "    def run(query: Annotated[str, SQL]) -> None: ...\n"
    )
    index = _core.module_index(VERSION, [("add", "m", source, "m", [], None)])
    helper = ("query", None, "static", None, None, None, False, None)
    assert _core.index_helpers(index)["m"]["C.run"][2:] == helper
    assert _core.index_helpers(_core.index_extended(index, [])) == _core.index_helpers(index)


def test_unparsable_module() -> None:
    assert _core.module_helpers(UNPARSABLE, VERSION) == {}
    assert _core.module_calls(UNPARSABLE, version=VERSION) == []
    assert _core.row_diagnostics(UNPARSABLE, version=VERSION) is None


@pytest.mark.parametrize("source", ["x = 1\n", "from sqlty import SQL\n"])
def test_row_diagnostics_of_a_module_without_queries(source: str) -> None:
    assert _core.row_diagnostics(source, version=VERSION) == ([], [], [])


@pytest.mark.parametrize("split", [_core.check_queries, _core.named_queries])
def test_unknown_query_format(split: Callable[..., object]) -> None:
    with pytest.raises(ValueError, match="unknown query-format 'bogus'"):
        split("SELECT 1", query_format="bogus")


CONFIG = ("postgres", None, None, None, False, [], None, None)
CHECK = (_core.check_modules, ("x = 1\n", None, 3))
BREAKING = (_core.breaking_modules, ("x = 1\n", None, 0, 3))


@pytest.mark.parametrize(("check", "module"), [CHECK, BREAKING])
def test_module_with_a_missing_configuration(
    check: Callable[..., object], module: tuple[object, ...]
) -> None:
    with pytest.raises(ValueError, match="no configuration 3"):
        check([module], [CONFIG], version=VERSION)


@pytest.mark.parametrize(("check", "module"), [CHECK, BREAKING])
def test_modules_with_an_unknown_import_style(
    check: Callable[..., object], module: tuple[object, ...]
) -> None:
    config = CONFIG[:3] + ("bogus",) + CONFIG[4:]
    with pytest.raises(ValueError, match="bogus"):
        check([module[:-1] + (0,)], [config], version=VERSION)


def test_cursor_defaults() -> None:
    found = _core.query_at(QUERY, 15)
    assert found is not None
    assert (found.text, found.offset, found.route) == ("--sql\nSELECT 1", 9, "marker")
    assert _core.query_source_offset(found, 9) == QUERY.index("ECT")
    assert _core.hover_source(QUERY, 18) is None
    assert _core.highlight_source(QUERY) == [
        (5, 10, "comment"),
        (12, 18, "keyword"),
        (19, 20, "number"),
    ]
    source = 'q = "--sql\\nSELECT * FROM "\n'
    found = _core.complete_source(source, 26, schema_sql="CREATE VIEW v AS SELECT 1 AS a;")
    assert found is not None
    assert found[3][0] == ("v", "view", "public", None, None)


def test_completion_of_a_cte() -> None:
    source = 'q = "--sql\\nWITH c AS (SELECT 1 AS a) SELECT * FROM "\n'
    found = _core.complete_source(source, len(source) - 2, limit=1)
    assert found is not None
    assert found[3] == [("c", "cte", None, None, None)]


@pytest.mark.parametrize(
    ("sql", "labels"),
    [
        ("SELECT | FROM p, (SELECT 1 AS a)", ["a", "p.id"]),
        ("SELECT | FROM p n, nope m WHERE (", ["id", "name"]),
    ],
)
def test_completion_of_unnamed_and_unknown_relations(sql: str, labels: list[str]) -> None:
    source = 'q = "--sql\\n' + sql.replace("|", "") + '"\n'
    found = _core.complete_source(
        source,
        source.index("SELECT ") + 7,
        schema_sql="CREATE TABLE p (id INT, name TEXT);",
        limit=2,
    )
    assert found is not None
    assert [label for label, *_ in found[3]] == labels

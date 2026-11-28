"""Edges of the ``sqlty._core`` bindings no other test reaches."""

import sys

import pytest

from sqlty import _core

VERSION = sys.version_info[:2]
UNPARSABLE = "x = (\n"
QUERY = 'q = "--sql\\nSELECT 1"\n'


@pytest.mark.parametrize(
    ("dialect", "checks"),
    [("postgres", True), ("SQLite", True), ("duckdb", False)],
)
def test_checks_names(dialect: str, checks: bool) -> None:
    assert _core.checks_names(dialect) is checks


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


def test_unknown_query_format() -> None:
    with pytest.raises(ValueError, match="unknown query-format 'bogus'"):
        _core.check_queries("SELECT 1", query_format="bogus")


def test_check_queries() -> None:
    found = _core.check_queries(
        "-- name: get-user^\nSELECT nope FROM users", schema_sql="CREATE TABLE users (id INT)"
    )
    assert [(diag["rule"], diag["line"]) for diag in found] == [("unknown-column", 2)]


def test_driver_types() -> None:
    assert ("INT4RANGE", "psycopg.types.range.Range[int]") in _core.driver_types(
        "psycopg", "postgres"
    )


CONFIG = ("postgres", None, None, None, False, [], None, None)
MODULE = ("x = 1\n", None, 3)


def test_module_with_a_missing_configuration() -> None:
    with pytest.raises(ValueError, match="no configuration 3"):
        _core.check_modules([MODULE], [CONFIG], version=VERSION)


def test_modules_with_an_unknown_import_style() -> None:
    config = CONFIG[:3] + ("bogus",) + CONFIG[4:]
    with pytest.raises(ValueError, match="bogus"):
        _core.check_modules([MODULE[:-1] + (0,)], [config], version=VERSION)


def test_cursor_defaults() -> None:
    assert _core.complete_source(QUERY, 15) == (15, 15, False, [])
    assert _core.hover_source(QUERY, 18) is None
    assert _core.highlight_source(QUERY) == [
        (5, 10, "comment", []),
        (12, 18, "keyword", []),
        (19, 20, "number", []),
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


def test_join_completion_and_hover() -> None:
    schema = "CREATE TABLE a (id INT PRIMARY KEY); CREATE TABLE b (a_id INT REFERENCES a (id));"
    source = 'q = "--sql\\nSELECT 1 FROM a JOIN "\n'
    found = _core.complete_source(source, len(source) - 2, schema_sql=schema)
    assert found is not None
    assert "join" in {kind for _, kind, *_ in found[3]}
    source = 'q = "--sql\\nSELECT id FROM a"\n'
    hovered = _core.hover_source(source, source.index("id"), schema_sql=schema)
    assert hovered is not None
    assert "id" in hovered[2]


def test_check_migrations() -> None:
    found = _core.check_migrations("CREATE TABLE t (a INT);", ["DROP TABLE t;"])
    assert [diag["rule"] for diag in found[0]] == ["missing-lock-timeout"]
    with pytest.raises(ValueError, match="unknown rule"):
        _core.check_migrations("", [], rules={"nope": "warn"})

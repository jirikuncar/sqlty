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
    assert (schema.dialect, schema.absent, repr(schema)) == (
        "postgres",
        False,
        'Schema(dialect="postgres")',
    )


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
    assert index.helpers()["m"]["C.run"][2:] == helper
    assert index.extended([]).helpers() == index.helpers()


def test_unparsable_module() -> None:
    assert _core.module_helpers(UNPARSABLE, VERSION) == {}
    assert _core.module_calls(UNPARSABLE, version=VERSION) == []
    assert _core.row_diagnostics(UNPARSABLE, version=VERSION) is None


def test_row_diagnostics_of_a_module_without_queries() -> None:
    assert _core.row_diagnostics("x = 1\n", version=VERSION) == ([], [], [])


@pytest.mark.parametrize("split", [_core.check_queries, _core.named_queries])
def test_unknown_query_format(split: Callable[..., object]) -> None:
    with pytest.raises(ValueError, match="unknown query-format 'bogus'"):
        split("SELECT 1", query_format="bogus")


def test_module_with_a_missing_configuration() -> None:
    with pytest.raises(ValueError, match="no configuration 3"):
        _core.check_modules([("x = 1\n", None, 3)], [], version=VERSION)


def test_modules_with_an_unknown_import_style() -> None:
    config = ("postgres", None, None, "bogus", False, [], None, None)
    with pytest.raises(ValueError, match="bogus"):
        _core.check_modules([("x = 1\n", None, 0)], [config], version=VERSION)


def test_cursor_defaults() -> None:
    found = _core.query_at(QUERY, 15)
    assert repr(found) == 'QueryAt(text="--sql\\nSELECT 1", offset=9, route="marker")'
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

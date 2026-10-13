"""How the engine reads SQL types and statements across dialects."""

from __future__ import annotations

import re
from pathlib import Path

import pytest

import sqlty.core as core


class TestParser:
    @pytest.mark.parametrize(
        ("sql", "dialect", "expected"),
        [
            ("SELECT x::ARRAY FROM t", "postgres", None),
            ("SELECT CAST(x AS ARRAY) FROM t", "mysql", None),
            (
                "SELECT x::INT[3], y::TEXT[2][2], z::INT ARRAY[3] FROM t",
                "postgres",
                "tuple[list[int], list[list[str]], list[int]]",
            ),
            ("SELECT (x::INT[])[1] FROM t", "postgres", "tuple[int]"),
            ("SELECT x::INT[y] FROM t", "postgres", None),
            ("SELECT x::INT[1+1] FROM t", "postgres", None),
            ("SELECT x::INT ARRAY, y::TEXT FROM t", "postgres", "tuple[list[int], str]"),
            ('SELECT "CAST"(x), "LENGTH"(y), LENGTH(z) FROM t', "postgres", None),
            ("SELECT CAST(x AS INT[]) FROM t", "mysql", None),
            ("SELECT CAST(x AS UNSIGNED ARRAY) FROM t", "mysql", "tuple[list[int]]"),
            ("SELECT CAST(x AS SET('a')) FROM t", "postgres", None),
            ("SELECT CAST(x AS SIGNED INTEGER) FROM t", "sqlite", None),
            ("SELECT CAST(x AS UNSIGNED INTEGER) FROM t", "mysql", "tuple[int]"),
            ("SELECT CAST(x AS INT3), CAST(y AS MIDDLEINT) FROM t", "mysql", "tuple[int, int]"),
            (
                "SELECT CAST(x AS INT SIGNED), CAST(y AS FLOAT UNSIGNED) FROM t",
                "mysql",
                "tuple[int, float]",
            ),
            ("SELECT CAST(x AS POINT) FROM t", "mysql", None),
            ("SELECT CAST(x AS DEC(8,3) UNSIGNED) FROM t", "mysql", "tuple[decimal.Decimal]"),
            ("SELECT CAST(x AS NUMERIC(8,3) UNSIGNED) FROM t", "mysql", None),
            ("SELECT CAST(x AS FIXED UNSIGNED) FROM t", "mysql", None),
            (
                "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE "
                "RETURNING merge_action(), t.id::INT",
                "postgres",
                None,
            ),
            ("MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE", "postgres", None),
        ],
    )
    def test_infer_type(self, sql: str, dialect: str, expected: str | None):
        assert core.infer_type(sql, dialect=dialect) == expected

    @pytest.mark.parametrize(
        ("column_types", "dialect", "expected"),
        [
            (
                "INT3, MIDDLEINT, INT8, INT(11) SIGNED, BIGINT(20) UNSIGNED, FLOAT UNSIGNED",
                "mysql",
                "c0::MEDIUMINT, c1::MEDIUMINT, c2::BIGINT, c3::INT(11), c4::UBIGINT(20), c5::UDOUBLE",
            ),
            (
                "YEAR, BIT(8), DATETIME(6), TIMESTAMP, ENUM('a'), SET('a'), JSON, LONGBLOB, TINYTEXT, POINT",
                "mysql",
                "c0::YEAR, c1::BIT(8), c2::DATETIME(6), c3::TIMESTAMPTZ, c4::ENUM('A'), "
                "c5::SET('A'), c6::JSON, c7::BLOB, c8::TEXT, c9::POINT",
            ),
            ("INT8, INT", "sqlite", "c0::BIGINT, c1::INT"),
            (
                "INT[3][3], INT ARRAY[4], TEXT[]",
                "postgres",
                "c0::ARRAY<ARRAY<INT>[3]>[3], c1::ARRAY<INT>[4], c2::ARRAY<TEXT>",
            ),
            ("INT ARRAY, INT", "postgres", "c0::ARRAY<INT>, c1::INT"),
        ],
    )
    def test_schema_types(self, column_types: str, dialect: str, expected: str):
        columns = ", ".join(f"c{i} {t}" for i, t in enumerate(column_types.split(", ")))
        schema = f"CREATE TABLE t ({columns});"
        result = core.expand_star("SELECT * FROM t", dialect=dialect, schema_sql=schema)
        assert result == f"SELECT {expected} FROM t"

    @pytest.mark.parametrize("dialect", ["postgres", "sqlite"])
    def test_mysql_only_column_skips_table(self, dialect: str):
        schema = "CREATE TABLE t (a INT, b SET('x'));"
        assert core.infer_type("SELECT a FROM t", dialect=dialect, schema_sql=schema) is None

    def test_int3_unsigned_column(self):
        schema = "CREATE TABLE t (a INT, b INT3 UNSIGNED);"
        assert core.expand_star("SELECT * FROM t", dialect="mysql", schema_sql=schema) == (
            "SELECT a::INT, b::UMEDIUMINT FROM t"
        )

    def test_merge_without_returning_not_wrapped(self):
        source = 'q = "--sql\\nMERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE"'
        assert core.rewrite_source(source, untyped=True) == source


class TestQueryInput:
    """The ``--sql`` marker and driver placeholders are read alike before parsing."""

    SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"

    @pytest.mark.parametrize(
        ("sql", "dialect", "expected"),
        [
            ("--sql\r\nSELECT id, name FROM users", "mysql", "tuple[int, str]"),
            ("  --sql\nSELECT id, name FROM users", "mysql", "tuple[int, str]"),
            ("--sql SELECT id FROM users", "mysql", None),
            ("SELECT id FROM users WHERE name = %b AND id = %(n)t", "postgres", "tuple[int]"),
            ("SELECT id FROM users WHERE id = %(a-b)s", "postgres", "tuple[int]"),
            ("SELECT id, 5 %% 2 FROM users WHERE name LIKE 'a%%'", "postgres", "tuple[int, int]"),
            (
                "SELECT $$it's %s$$::TEXT, id FROM users WHERE id = %s",
                "postgres",
                "tuple[str, int]",
            ),
            (
                "SELECT E'it''s %s'::TEXT, id FROM users WHERE id = %s",
                "postgres",
                "tuple[str, int]",
            ),
            ("SELECT id FROM users WHERE id = %s", "sqlite", "tuple[int]"),
        ],
    )
    def test_infer_type(self, sql: str, dialect: str, expected: str | None):
        assert core.infer_type(sql, dialect=dialect, schema_sql=self.SCHEMA) == expected

    def test_schema_is_not_a_query(self):
        """Placeholders are not replaced in schema DDL, where MySQL rejects them."""
        schema = "CREATE TABLE t (a INT DEFAULT %s);"
        assert core.expand_star("SELECT * FROM t", dialect="mysql", schema_sql=schema) is None


def _type_table() -> dict[str, str]:
    """``SQL_TO_PYTHON_TYPE`` of the analyzer, without ``ENUM`` (its values are needed to parse)."""
    source = (Path(__file__).resolve().parent.parent / "rust/src/analyzer.rs").read_text()
    constants = dict(re.findall(r'const (\w+): &str = "([\w. |]+)";', source))
    table = source[
        source.index("static SQL_TO_PYTHON_TYPE") : source.index("static FUNCTION_TYPES")
    ]
    pairs = re.findall(r'\("(\w+)", (?:"([\w.]+)"|(\w+))\)', table)
    return {name: literal or constants[const] for name, literal, const in pairs if name != "ENUM"}


@pytest.mark.parametrize("dialect", ["postgres", "mysql", "sqlite"])
@pytest.mark.parametrize(("sql_type", "python_type"), sorted(_type_table().items()))
def test_type_names(sql_type: str, python_type: str, dialect: str):
    """A mapped type name, cast or in a schema, is its Python type."""
    schema = f"CREATE TABLE t (c {sql_type});"
    cast = core.infer_type(f"SELECT CAST(x AS {sql_type}) FROM t", dialect=dialect)
    column = core.infer_type("SELECT c FROM t", dialect=dialect, schema_sql=schema)
    assert cast == column == f"tuple[{python_type}]"
    expanded = core.expand_star("SELECT * FROM t", dialect=dialect, schema_sql=schema)
    assert expanded is not None and expanded.startswith("SELECT c::")


@pytest.mark.parametrize(
    ("sql", "expected"),
    [
        ("SELECT 1", True),
        ("DELETE FROM t RETURNING id", True),
        ("DELETE FROM t", False),
        ("CREATE TABLE t (a INT)", False),
        ("NOT VALID SQL", True),
        ("", False),
        ("SELECT id FROM t WHERE id = ?", True),
    ],
)
def test_returns_rows(sql: str, expected: bool):
    assert core.returns_rows(sql) is expected


@pytest.mark.parametrize("imports", ["", "import not_installed\n"])
def test_check_source_typeddict_inheritance_lines(imports: str):
    """typeddict-inheritance errors sit on the offending subclass's class line."""
    from sqlty.core import check_source

    source = imports + (
        "from typing import TypedDict\n"
        "from sqlty import SQL\n"
        "class Base(TypedDict, closed=True):\n"
        "    id: int\n"
        "class Middle(Base):\n"
        "    name: str\n"
        "class User(Middle):\n"
        "    pass\n"
        "class Other(Base, closed=False):\n"
        "    pass\n"
        'q = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")\n'
        'r = SQL[Other]("--sql\\nSELECT id::INTEGER FROM users")\n'
    )
    offset = len(imports.splitlines())
    assert [
        (d["line"], d["message"])
        for d in check_source(source, dialect="postgres")
        if d["rule"] == "typeddict-inheritance"
    ] == [
        (5 + offset, "Middle cannot add key 'name' to closed base Base"),
        (9 + offset, "Other cannot set closed=False: base Base is closed"),
    ]

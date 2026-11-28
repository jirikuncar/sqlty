"""How the engine reads SQL types and statements across dialects."""

from __future__ import annotations

import re
from pathlib import Path

import pytest

import sqlty.core as core
from tests._json_shapes import json_shapes


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
            ("SELECT (x::INT[])[1] FROM t", "postgres", "tuple[int | None]"),
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
            ("SELECT CAST(x AS POINT) FROM t", "mysql", "tuple[bytes]"),
            ("SELECT CAST(x AS DEC(8,3) UNSIGNED) FROM t", "mysql", "tuple[decimal.Decimal]"),
            ("SELECT CAST(x AS NUMERIC(8,3) UNSIGNED) FROM t", "mysql", None),
            ("SELECT CAST(x AS FIXED UNSIGNED) FROM t", "mysql", None),
            ("SELECT CAST(x AS INT ZEROFILL) FROM t", "mysql", "tuple[int]"),
            ("SELECT CAST(x AS SOMEBLOB) FROM t", "sqlite", "tuple[bytes]"),
            ("SELECT CAST(x AS FLOATING) FROM t", "sqlite", "tuple[float]"),
            (
                "SELECT CAST(x AS TIMESTAMP WITHOUT TIME ZONE) FROM t",
                "postgres",
                "tuple[datetime.datetime]",
            ),
            ("SELECT CAST(x AS UInt8) FROM t", "clickhouse", "tuple[int]"),
            ("SELECT CAST(x AS UInt16) FROM t", "clickhouse", "tuple[int]"),
            ("SELECT CAST(x AS UInt32) FROM t", "clickhouse", "tuple[int]"),
            ("SELECT CAST(x AS UHUGEINT) FROM t", "duckdb", "tuple[int]"),
            ("SELECT CAST(x AS BOOL) FROM t", "postgres", "tuple[bool]"),
            ("SELECT CAST(x AS BINARY LARGE OBJECT) FROM t", "generic", "tuple[bytes]"),
            ("SELECT CAST(x AS FLOAT(10)) FROM t", "postgres", "tuple[float]"),
            ("SELECT CAST(x AS FLOAT(53)) FROM t", "postgres", "tuple[float]"),
            (
                "SELECT CAST(x AS STRUCT(a INTEGER, b BIGINT)) FROM t",
                "duckdb",
                "tuple[dict[str, int]]",
            ),
            (
                "SELECT CAST(x AS STRUCT<a ANY TYPE>) FROM t",
                "bigquery",
                "tuple[dict[str, typing.Any]]",
            ),
            (
                "SELECT CAST(x AS STRUCT(a TABLE(b INT))) FROM t",
                "duckdb",
                "tuple[dict[str, typing.Any]]",
            ),
            (
                "SELECT CAST(x AS STRUCT(a INTEGER[])) FROM t",
                "duckdb",
                "tuple[dict[str, list[int]]]",
            ),
            (
                "SELECT CAST(x AS STRUCT(a INTEGER[3])) FROM t",
                "duckdb",
                "tuple[dict[str, tuple[int, ...]]]",
            ),
            (
                "SELECT CAST(x AS STRUCT<a ARRAY<INT64>>) FROM t",
                "bigquery",
                "tuple[dict[str, list[int]]]",
            ),
            (
                "SELECT CAST(x AS Tuple(a Array(Int32))) FROM t",
                "clickhouse",
                "tuple[tuple[list[int]]]",
            ),
            ("SELECT CAST(x AS MAP(VARCHAR, ARRAY)) FROM t", "duckdb", "tuple[dict[str, list]]"),
            ("SELECT CAST(x AS ARRAY) FROM t", "snowflake", "tuple[list]"),
            (
                "SELECT CAST(x AS STRUCT(a INTEGER ARRAY)) FROM t",
                "duckdb",
                "tuple[dict[str, list[int]]]",
            ),
            (
                "SELECT CAST(x AS STRUCT<a: ARRAY<INT>>) FROM t",
                "databricks",
                "tuple[dict[str, list[int]]]",
            ),
            (
                "SELECT CAST(x AS STRUCT<a INT64, b STRING>) FROM t",
                "bigquery",
                "tuple[dict[str, int | str]]",
            ),
            (
                "SELECT CAST(x AS STRUCT<INT64, STRING>) FROM t",
                "bigquery",
                "tuple[dict[str, int | str]]",
            ),
            (
                "SELECT CAST(x AS ARRAY<STRUCT<a INT64>>) FROM t",
                "bigquery",
                "tuple[list[dict[str, int]]]",
            ),
            ("SELECT CAST(x AS Array(Int32)) FROM t", "clickhouse", "tuple[list[int]]"),
            ("SELECT CAST(x AS Nullable(String)) FROM t", "clickhouse", "tuple[str]"),
            ("SELECT CAST(x AS LowCardinality(String)) FROM t", "clickhouse", "tuple[str]"),
            (
                "SELECT CAST(x AS Tuple(a Int32, b String)) FROM t",
                "clickhouse",
                "tuple[tuple[int, str]]",
            ),
            (
                "SELECT CAST(x AS Tuple(Int32, String)) FROM t",
                "clickhouse",
                "tuple[tuple[int, str]]",
            ),
            ("SELECT CAST(x AS Map(String, Int64)) FROM t", "clickhouse", "tuple[dict[str, int]]"),
            ("SELECT CAST(x AS Nested(a Int32)) FROM t", "clickhouse", None),
            ("SELECT CAST(x AS INTEGER[3]) FROM t", "duckdb", "tuple[tuple[int, ...]]"),
            ("SELECT CAST(x AS UNION(a INTEGER, b VARCHAR)) FROM t", "duckdb", "tuple[int | str]"),
            ("SELECT CAST(x AS MAP(VARCHAR, INTEGER)) FROM t", "duckdb", "tuple[dict[str, int]]"),
            ("SELECT CAST(x AS MAP(INTEGER[], INTEGER)) FROM t", "duckdb", None),
            ("SELECT CAST(x AS STRUCT()) FROM t", "duckdb", None),
            ("SELECT CAST(x AS UNION(a INTEGER, b WHATEVER)) FROM t", "duckdb", None),
            (
                "SELECT CAST(x AS STRUCT(a WHATEVER)) FROM t",
                "duckdb",
                "tuple[dict[str, typing.Any]]",
            ),
            ("SELECT CAST(x AS WHATEVER[3]) FROM t", "duckdb", "tuple[tuple[typing.Any, ...]]"),
            ("SELECT CAST(x AS ROW(a INT, b TEXT)) FROM t", "postgres", "tuple[tuple[int, str]]"),
            ("SELECT CAST(x AS MAP(TEXT, INT)) FROM t", "postgres", "tuple[dict[str, int]]"),
            ("SELECT CAST(x AS STRUCT(a INT)) FROM t", "postgres", "tuple[dict[str, int]]"),
            ("SELECT CAST(x AS UNION(a INT)) FROM t", "postgres", "tuple[int]"),
            ("SELECT CAST(x AS MAP(TEXT)) FROM t", "postgres", None),
            ("SELECT CAST(x AS INT4RANGE) FROM t", "postgres", None),
            ("SELECT CAST(x AS INTERVAL) FROM t", "postgres", "tuple[datetime.timedelta]"),
            ("SELECT CAST(x AS OBJECT) FROM t", "snowflake", None),
            ("SELECT CAST(x AS MAP<STRING, INT>) FROM t", "hive", "tuple[dict[str, int]]"),
            ("SELECT CAST(x AS TABLE(a INT)) FROM t", "generic", None),
            (
                "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE "
                "RETURNING merge_action(), t.id::INT",
                "postgres",
                "tuple[str, int]",
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
                "c0::YEAR, c1::BIT(8), c2::DATETIME(6), c3::TIMESTAMPTZ, c4::ENUM('a'), "
                "c5::SET('a'), c6::JSON, c7::BLOB, c8::TEXT, c9::POINT",
            ),
            ("INT8, INT", "sqlite", "c0::BIGINT, c1::INT"),
            ("STRING COLLATE UTF8_LCASE", "databricks", "c0::TEXT"),
            (
                "INT(5) ZEROFILL, VARCHAR(3) COLLATE utf8mb4_bin",
                "mysql",
                "c0::UINT(5), c1::VARCHAR(3)",
            ),
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
        columns = core.analyze_select("SELECT * FROM t", dialect=dialect, schema_sql=schema)
        assert columns is not None
        assert ", ".join(f"{c.name}::{c.sql_type}" for c in columns) == expected

    @pytest.mark.parametrize("dialect", ["postgres", "sqlite"])
    def test_mysql_only_column_skips_table(self, dialect: str):
        schema = "CREATE TABLE t (a INT, b SET('x'));"
        assert core.infer_type("SELECT a FROM t", dialect=dialect, schema_sql=schema) is None

    def test_int3_unsigned_column(self):
        schema = "CREATE TABLE t (a INT, b INT3 UNSIGNED);"
        columns = core.analyze_select("SELECT * FROM t", dialect="mysql", schema_sql=schema)
        assert columns is not None
        assert [c.sql_type for c in columns] == ["INT", "UMEDIUMINT"]

    def test_merge_without_returning_not_wrapped(self):
        source = 'q = "--sql\\nMERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE"'
        assert core.rewrite_source(source, core.Settings(untyped=True)) == source


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
    source = (
        Path(__file__).resolve().parent.parent / "rust/crates/sqlty-sql/src/analyzer/sql_type.rs"
    ).read_text()
    constants = dict(re.findall(r'const (\w+): &str = "([\w. |]+)";', source))
    start = source.index("static SQL_TO_PYTHON_TYPE")
    table = source[start : source.index("\n});", start)]
    pairs = re.findall(r'\("(\w+)", (?:"([\w.]+)"|(\w+))\)', table)
    return {name: literal or constants[const] for name, literal, const in pairs if name != "ENUM"}


def _postgres_extension_types() -> set[str]:
    """``POSTGRES_EXTENSION_TYPES`` of the analyzer: mapped in PostgreSQL only."""
    source = (
        Path(__file__).resolve().parent.parent / "rust/crates/sqlty-sql/src/analyzer/sql_type.rs"
    ).read_text()
    start = source.index("const POSTGRES_EXTENSION_TYPES")
    return set(re.findall(r'"(\w+)"', source[start : source.index("];", start)]))


@pytest.mark.parametrize(
    ("spelling", "dialect", "sql_type"),
    [
        ("REAL", "generic", "FLOAT"),
        ("BLOB", "generic", "VARBINARY"),
        ("TIME WITH TIME ZONE", "generic", "TIMETZ"),
        ("TIME WITHOUT TIME ZONE", "generic", "TIME"),
        ("INT1", "generic", "TINYINT"),
        ("INT2", "generic", "SMALLINT"),
        ("INT16", "generic", "SMALLINT"),
        ("INT32", "generic", "INT"),
        ("INT8", "generic", "BIGINT"),
        ("INT64", "generic", "BIGINT"),
        ("UINT64", "generic", "UBIGINT"),
        ("HUGEINT", "generic", "INT128"),
        ("FLOAT32", "generic", "FLOAT"),
        ("FLOAT64", "generic", "DOUBLE"),
        ("NUMBER", "generic", "DECIMAL"),
        ("BIGNUMERIC", "generic", "BIGDECIMAL"),
        ("VARCHAR2", "generic", "VARCHAR"),
        ("NVARCHAR2", "generic", "VARCHAR"),
        ("CLOB", "generic", "TEXT"),
        ("UNIQUEIDENTIFIER", "generic", "UUID"),
        ("DATETIME2", "generic", "TIMESTAMP"),
        ("TIMESTAMP_TZ", "generic", "TIMESTAMPTZ"),
        ("TIMESTAMP_LTZ", "generic", "TIMESTAMPTZ"),
        ("DATETIMEOFFSET", "generic", "TIMESTAMPTZ"),
        ("FLOAT8", "generic", "DOUBLE"),
        ("FLOAT4", "generic", "FLOAT"),
        ("DEC", "generic", "DECIMAL"),
        ("FIXED", "generic", "DECIMAL"),
        ("CHARACTER VARYING", "generic", "VARCHAR"),
        ("NVARCHAR", "generic", "VARCHAR"),
        ("CHARACTER LARGE OBJECT", "generic", "TEXT"),
        ("CHAR LARGE OBJECT", "generic", "TEXT"),
        ("CHARACTER", "generic", "CHAR"),
        ("NCHAR", "generic", "CHAR"),
        ("TINYTEXT", "generic", "TEXT"),
        ("TINYBLOB", "generic", "BLOB"),
        ("STRING", "generic", "TEXT"),
        ("BYTEA", "generic", "VARBINARY"),
        ("INTEGER(10)", "mysql", "INT(10)"),
        ("BYTES", "bigquery", "BINARY"),
        ("BINARY LARGE OBJECT", "generic", "VARBINARY"),
        ("BIT VARYING(3)", "postgres", "VARBIT(3)"),
        ("REAL", "mysql", "DOUBLE"),
        ("BLOB", "mysql", "BLOB"),
        ("INT8", "clickhouse", "TINYINT"),
        ("DATETIME2", "mysql", "TIMESTAMPTZ"),
    ],
)
def test_type_spellings(spelling: str, dialect: str, sql_type: str):
    """Each dialect's spelling of a type reads as the one canonical name."""
    query = f"SELECT CAST(x AS {spelling}) AS c FROM t"
    [column] = core.analyze_select(query, dialect=dialect)
    assert column.sql_type == sql_type


@pytest.mark.parametrize("dialect", ["postgres", "mysql", "sqlite"])
@pytest.mark.parametrize(("sql_type", "python_type"), sorted(_type_table().items()))
def test_type_names(sql_type: str, python_type: str, dialect: str):
    """A mapped type name, cast or in a schema, is its Python type."""
    if (sql_type, dialect) == ("TIME", "mysql"):
        python_type = "datetime.timedelta"
    if dialect == "sqlite" and python_type in (
        "bool",
        "datetime.date",
        "datetime.time",
        "datetime.datetime",
        "uuid.UUID",
    ):
        # sqlite3 returns stored integers and text: SQLite has no such storage class.
        python_type = "int" if python_type == "bool" else "str"
    schema = f"CREATE TABLE t (c {sql_type} NOT NULL);"
    cast = core.infer_type(f"SELECT CAST(x AS {sql_type}) FROM t", dialect=dialect)
    column = core.infer_type("SELECT c FROM t", dialect=dialect, schema_sql=schema)
    if dialect != "postgres" and sql_type in _postgres_extension_types():
        assert column != f"tuple[{python_type}]"
        return
    assert cast == column == f"tuple[{python_type}]"
    expanded = core.expand_star("SELECT * FROM t", dialect=dialect, schema_sql=schema)
    assert expanded is not None
    if dialect == "postgres":
        assert expanded.startswith("SELECT c::")
    else:
        assert expanded == "SELECT c FROM t"


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
    from sqlty.core import Settings, check_source

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
        for d in check_source(source, Settings(dialect="postgres"))
        if d["rule"] == "typeddict-inheritance"
    ] == [
        (5 + offset, "Middle cannot add key 'name' to closed base Base"),
        (9 + offset, "Other cannot set closed=False: base Base is closed"),
    ]


def test_json_shapes_of_a_broken_query_is_empty() -> None:
    assert json_shapes("SELEC (") == []

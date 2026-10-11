"""Both backends parse types and statements alike (sqlglot vs sqlparser-rs)."""

from __future__ import annotations

import types

import pytest


def _outcome(call, backend: types.ModuleType):
    """A call's result on ``backend``, or its exception type and message."""
    try:
        return call(backend)
    except Exception as exc:
        return type(exc).__name__, str(exc)


class TestParserParity:
    """Both backends parse types and statements alike (sqlglot vs sqlparser-rs)."""

    @pytest.mark.parametrize(
        ("sql", "dialect", "expected"),
        [
            ("SELECT x::ARRAY FROM t", "postgres", None),
            ("SELECT CAST(x AS ARRAY) FROM t", "mysql", None),
            (
                "SELECT x::INT[3], y::TEXT[2][2], z::INT ARRAY[3] FROM t",
                "postgres",
                "tuple[list, list, list]",
            ),
            ("SELECT (x::INT[])[1] FROM t", "postgres", "tuple[str]"),
            ("SELECT x::INT[y] FROM t", "postgres", None),
            ("SELECT x::INT[1+1] FROM t", "postgres", None),
            ("SELECT x::INT ARRAY, y FROM t", "postgres", "tuple[list, str]"),
            ('SELECT "CAST"(x), "LENGTH"(y), LENGTH(z) FROM t', "postgres", "tuple[str, str, int]"),
            ("SELECT CAST(x AS INT[]) FROM t", "mysql", None),
            ("SELECT CAST(x AS UNSIGNED ARRAY) FROM t", "mysql", "tuple[list]"),
            ("SELECT CAST(x AS SET('a')) FROM t", "postgres", None),
            ("SELECT CAST(x AS SIGNED INTEGER) FROM t", "sqlite", None),
            ("SELECT CAST(x AS UNSIGNED INTEGER) FROM t", "mysql", "tuple[int]"),
            ("SELECT CAST(x AS INT3), CAST(y AS MIDDLEINT) FROM t", "mysql", "tuple[int, int]"),
            (
                "SELECT CAST(x AS INT SIGNED), CAST(y AS FLOAT UNSIGNED) FROM t",
                "mysql",
                "tuple[int, float]",
            ),
            ("SELECT CAST(x AS POINT) FROM t", "mysql", "tuple[str]"),
            ("SELECT CAST(x AS DEC(8,3) UNSIGNED) FROM t", "mysql", "tuple[float]"),
            ("SELECT CAST(x AS NUMERIC(8,3) UNSIGNED) FROM t", "mysql", None),
            ("SELECT CAST(x AS FIXED UNSIGNED) FROM t", "mysql", None),
            (
                "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE "
                "RETURNING merge_action(), t.id::INT",
                "postgres",
                "tuple[str, int]",
            ),
            ("MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE", "postgres", None),
        ],
    )
    def test_infer_type(self, core: types.ModuleType, sql: str, dialect: str, expected: str | None):
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
    def test_schema_types(
        self, core: types.ModuleType, column_types: str, dialect: str, expected: str
    ):
        columns = ", ".join(f"c{i} {t}" for i, t in enumerate(column_types.split(", ")))
        schema = f"CREATE TABLE t ({columns});"
        result = core.expand_star("SELECT * FROM t", dialect=dialect, schema_sql=schema)
        assert result == f"SELECT {expected} FROM t"

    @pytest.mark.parametrize("dialect", ["postgres", "sqlite"])
    def test_mysql_only_column_skips_table(self, core: types.ModuleType, dialect: str):
        schema = "CREATE TABLE t (a INT, b SET('x'));"
        assert core.infer_type("SELECT a FROM t", dialect=dialect, schema_sql=schema) == (
            "tuple[str]"
        )

    @pytest.mark.xfail(
        strict=True,
        reason="sqlparser-rs has no INT3/MIDDLEINT keyword: `INT3 UNSIGNED` is a parse error, "
        "so Rust skips the table while Python reads b::UMEDIUMINT",
    )
    def test_int3_unsigned_column(self):
        rs = pytest.importorskip("sqlty._core", reason="Rust backend not compiled")
        import sqlty._core_py as py

        schema = "CREATE TABLE t (a INT, b INT3 UNSIGNED);"
        outcomes = {
            m.expand_star("SELECT * FROM t", dialect="mysql", schema_sql=schema) for m in (py, rs)
        }
        assert len(outcomes) == 1

    def test_merge_without_returning_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql\\nMERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE"'
        assert core.rewrite_source(source, untyped=True) == source


def test_type_tables_agree():
    """Rust's SQL_TO_PYTHON_TYPE lists the same types as SQLAnalyzer.SQL_TO_PYTHON_TYPE."""
    import re
    from pathlib import Path

    from sqlty._sql_analyzer import SQLAnalyzer

    source = (Path(__file__).resolve().parent.parent / "rust/src/analyzer.rs").read_text()
    table = source[
        source.index("static SQL_TO_PYTHON_TYPE") : source.index("static FUNCTION_TYPES")
    ]
    assert dict(re.findall(r'\("(\w+)", "([\w.]+)"\)', table)) == SQLAnalyzer.SQL_TO_PYTHON_TYPE


def _type_names() -> list[str]:
    from sqlty._sql_analyzer import SQLAnalyzer

    return sorted(SQLAnalyzer.SQL_TO_PYTHON_TYPE)


@pytest.mark.parametrize("dialect", ["postgres", "mysql", "sqlite"])
@pytest.mark.parametrize("sql_type", _type_names())
def test_type_names_agree(sql_type: str, dialect: str):
    """Every mapped type name casts and expands alike on both backends."""
    rs = pytest.importorskip("sqlty._core", reason="Rust backend not compiled")
    import sqlty._core_py as py

    schema = f"CREATE TABLE t (c {sql_type});"
    calls = [
        lambda m: m.infer_type(f"SELECT CAST(x AS {sql_type}) FROM t", dialect=dialect),
        lambda m: m.expand_star("SELECT * FROM t", dialect=dialect, schema_sql=schema),
        lambda m: m.infer_type("SELECT * FROM t", dialect=dialect, schema_sql=schema),
    ]
    for call in calls:
        assert _outcome(call, rs) == _outcome(call, py)


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

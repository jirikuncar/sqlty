"""Conformance tests for the core API contract.

Every function in ``sqlty.core`` must pass these tests identically, whether
backed by ``_core_py`` (pure Python) or ``_core`` (Rust via pyo3).

Run against a specific backend::

    SQLTY_LIB=py pytest tests/test_core_conformance.py -v
    SQLTY_LIB=rs pytest tests/test_core_conformance.py -v

The ``core`` fixture (from conftest.py) parametrises automatically with all
available backends unless ``SQLTY_LIB`` or ``--sqlty-lib`` restricts it.

The tests are organised by function and cover:
- Expected return types and values for representative inputs
- Edge-case handling (empty string, invalid SQL, None schema, …)
- Round-trip invariants (rewrite → strip → rewrite is stable)
- Idempotency (rewrite(rewrite(x)) == rewrite(x))
- String-content preservation (the rewriter never modifies string content)
"""

from __future__ import annotations

import types

import pytest

# ---------------------------------------------------------------------------
# infer_type
# ---------------------------------------------------------------------------


class TestInferType:
    """Contract: infer_type(sql, *, dialect, schema_sql) -> str | None."""

    def test_single_integer(self, core: types.ModuleType):
        assert core.infer_type("SELECT id::INTEGER FROM t") == "tuple[int]"

    def test_multiple_columns(self, core: types.ModuleType):
        assert core.infer_type("SELECT id::INTEGER, name::TEXT FROM t") == "tuple[int, str]"

    def test_float(self, core: types.ModuleType):
        assert core.infer_type("SELECT price::FLOAT FROM t") == "tuple[float]"

    def test_boolean(self, core: types.ModuleType):
        assert core.infer_type("SELECT active::BOOLEAN FROM t") == "tuple[bool]"

    def test_datetime(self, core: types.ModuleType):
        result = core.infer_type("SELECT ts::TIMESTAMP FROM t")
        assert result is not None
        assert "datetime" in result

    def test_uuid(self, core: types.ModuleType):
        result = core.infer_type("SELECT uid::UUID FROM t")
        assert result is not None
        assert "uuid" in result

    def test_returns_none_for_garbage(self, core: types.ModuleType):
        assert core.infer_type("not valid sql at all {{{}}") is None

    def test_returns_none_for_empty(self, core: types.ModuleType):
        assert core.infer_type("") is None

    def test_dialect_postgres(self, core: types.ModuleType):
        assert core.infer_type("SELECT 1::INTEGER", dialect="postgres") == "tuple[int]"

    def test_dialect_mysql(self, core: types.ModuleType):
        result = core.infer_type("SELECT CAST(1 AS SIGNED)", dialect="mysql")
        assert result is not None

    def test_dialect_sqlite(self, core: types.ModuleType):
        result = core.infer_type("SELECT CAST(1 AS INTEGER)", dialect="sqlite")
        assert result is not None

    def test_schema_aware(self, core: types.ModuleType):
        schema = "CREATE TABLE users (id INTEGER, name TEXT);"
        result = core.infer_type("SELECT id, name FROM users", schema_sql=schema)
        assert result is not None
        assert "int" in result
        assert "str" in result

    def test_schema_none(self, core: types.ModuleType):
        # Without schema, uncast columns default to str
        result = core.infer_type("SELECT id FROM users")
        assert result is not None


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


# ---------------------------------------------------------------------------
# rewrite_source
# ---------------------------------------------------------------------------


class TestRewriteSource:
    """Contract: rewrite_source(source, *, dialect, schema_sql) -> str."""

    def test_wraps_sql_string(self, core: types.ModuleType):
        result = core.rewrite_source('q = "--sql\\nSELECT 1::INTEGER"')
        assert "SQL[tuple[int]]" in result

    def test_preserves_string_content(self, core: types.ModuleType):
        """Rewriter never modifies the string — only adds SQL[T]() wrapper."""
        result = core.rewrite_source('q = "--sql\\nSELECT 1::INTEGER"')
        assert '"--sql\\nSELECT 1::INTEGER"' in result

    def test_adds_sql_import(self, core: types.ModuleType):
        result = core.rewrite_source('q = "--sql\\nSELECT 1::INTEGER"')
        assert "from sqlty import SQL" in result

    def test_adds_datetime_import(self, core: types.ModuleType):
        result = core.rewrite_source('q = "--sql\\nSELECT ts::TIMESTAMP"')
        assert "import datetime" in result

    def test_adds_uuid_import(self, core: types.ModuleType):
        result = core.rewrite_source('q = "--sql\\nSELECT uid::UUID"')
        assert "import uuid" in result

    def test_non_sql_unchanged(self, core: types.ModuleType):
        source = 'x = 42\nname = "hello"'
        assert core.rewrite_source(source) == source

    def test_already_wrapped_unchanged(self, core: types.ModuleType):
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        assert core.rewrite_source(source) == source

    def test_idempotent(self, core: types.ModuleType):
        source = 'q = "--sql\\nSELECT id::INTEGER FROM t"'
        once = core.rewrite_source(source)
        twice = core.rewrite_source(once)
        assert once == twice

    def test_single_quotes(self, core: types.ModuleType):
        result = core.rewrite_source("q = '--sql\\nSELECT 1::INTEGER'")
        assert "SQL[tuple[int]]" in result

    def test_preserves_surrounding_code(self, core: types.ModuleType):
        source = 'x = 1\nq = "--sql\\nSELECT 1::INTEGER"\ny = 2'
        result = core.rewrite_source(source)
        assert "x = 1" in result
        assert "y = 2" in result

    def test_multiple_queries(self, core: types.ModuleType):
        source = 'a = "--sql\\nSELECT 1::INTEGER"\nb = "--sql\\nSELECT id::TEXT FROM t"\n'
        result = core.rewrite_source(source)
        assert "SQL[tuple[int]]" in result
        assert "SQL[tuple[str]]" in result

    def test_returns_str(self, core: types.ModuleType):
        assert isinstance(core.rewrite_source("x = 1"), str)

    # --- Corner cases: queries that should NOT be wrapped ---

    def test_invalid_sql_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql\\nNOT VALID SQL"'
        assert "SQL[" not in core.rewrite_source(source)

    def test_empty_query_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql"'
        assert "SQL[" not in core.rewrite_source(source)

    def test_whitespace_only_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql   "'
        assert "SQL[" not in core.rewrite_source(source)

    def test_update_without_returning_not_wrapped(self, core: types.ModuleType):
        source = "q = \"--sql\\nUPDATE users SET name = 'x'\""
        assert "SQL[" not in core.rewrite_source(source)

    def test_insert_without_returning_not_wrapped(self, core: types.ModuleType):
        source = """q = "--sql\\nINSERT INTO users (name) VALUES ('x')" """
        assert "SQL[" not in core.rewrite_source(source)

    def test_delete_without_returning_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql\\nDELETE FROM users"'
        assert "SQL[" not in core.rewrite_source(source)

    def test_create_table_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql\\nCREATE TABLE t (id INT)"'
        assert "SQL[" not in core.rewrite_source(source)

    def test_drop_table_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql\\nDROP TABLE users"'
        assert "SQL[" not in core.rewrite_source(source)

    def test_begin_not_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql\\nBEGIN"'
        assert "SQL[" not in core.rewrite_source(source)

    def test_dml_with_returning_is_wrapped(self, core: types.ModuleType):
        source = 'q = "--sql\\nUPDATE users SET x=1 RETURNING id::INTEGER"'
        result = core.rewrite_source(source)
        assert "SQL[tuple[int]]" in result

    def test_select_without_casts_is_wrapped(self, core: types.ModuleType):
        """SELECT without casts still wraps — type defaults to tuple[str, ...]."""
        source = 'q = "--sql\\nSELECT id, name FROM users"'
        result = core.rewrite_source(source)
        assert "SQL[tuple[str, str]]" in result

    def test_space_form_is_sql_comment_not_wrapped(self, core: types.ModuleType):
        """--sql SELECT ... with space is a SQL comment — parser sees empty query."""
        source = 'q = "--sql SELECT 1::INTEGER"'
        assert "SQL[" not in core.rewrite_source(source)


# ---------------------------------------------------------------------------
# strip_source
# ---------------------------------------------------------------------------


class TestStripSource:
    """Contract: strip_source(source) -> str."""

    def test_removes_wrapper(self, core: types.ModuleType):
        source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        result = core.strip_source(source)
        assert "SQL[" not in result
        assert '"--sql\\nSELECT 1::INTEGER"' in result

    def test_removes_sql_import(self, core: types.ModuleType):
        source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        result = core.strip_source(source)
        assert "from sqlty import SQL" not in result

    def test_preserves_manual_annotation(self, core: types.ModuleType):
        source = 'q = SQL[int]("SELECT 1")'
        assert core.strip_source(source) == source

    def test_preserves_non_sql(self, core: types.ModuleType):
        source = "x = 42"
        assert core.strip_source(source) == source

    def test_idempotent(self, core: types.ModuleType):
        source = 'q = "--sql\\nSELECT 1::INTEGER"'
        once = core.strip_source(source)
        twice = core.strip_source(once)
        assert once == twice

    def test_handles_space_format(self, core: types.ModuleType):
        # Legacy space format is also strippable
        source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'
        result = core.strip_source(source)
        assert "SQL[" not in result

    def test_returns_str(self, core: types.ModuleType):
        assert isinstance(core.strip_source("x = 1"), str)


# ---------------------------------------------------------------------------
# Round-trip: rewrite ↔ strip
# ---------------------------------------------------------------------------


class TestRoundTrip:
    """Contract: strip(rewrite(x)) produces a stable string; second cycle is identity."""

    def test_rewrite_then_strip_is_stable(self, core: types.ModuleType):
        original = 'q = "--sql\\nSELECT id::INTEGER, name::TEXT FROM t"'
        rewritten = core.rewrite_source(original)
        stripped = core.strip_source(rewritten)
        # Second cycle must be identity
        re_rewritten = core.rewrite_source(stripped)
        re_stripped = core.strip_source(re_rewritten)
        assert re_stripped == stripped

    def test_strip_then_rewrite_is_stable(self, core: types.ModuleType):
        wrapped = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        stripped = core.strip_source(wrapped)
        rewritten = core.rewrite_source(stripped)
        re_stripped = core.strip_source(rewritten)
        assert re_stripped == stripped

    def test_strip_rewrite_roundtrip_preserves_content(self, core: types.ModuleType):
        """strip(rewrite(source)) == source (ignoring imports)."""
        original = 'q = "--sql\\nSELECT id::INTEGER FROM users"'
        rewritten = core.rewrite_source(original)
        stripped = core.strip_source(rewritten)
        assert stripped.strip() == original.strip()


# ---------------------------------------------------------------------------
# expand_star
# ---------------------------------------------------------------------------


class TestExpandStar:
    """Contract: expand_star(sql, *, dialect, schema_sql) -> str | None."""

    def test_expands_with_schema(self, core: types.ModuleType):
        schema = "CREATE TABLE users (id INTEGER, name TEXT);"
        result = core.expand_star("SELECT * FROM users", schema_sql=schema)
        assert result is not None
        assert "id" in result
        assert "name" in result
        assert "*" not in result

    def test_returns_none_without_schema(self, core: types.ModuleType):
        assert core.expand_star("SELECT * FROM users") is None

    def test_non_star_returns_none(self, core: types.ModuleType):
        schema = "CREATE TABLE users (id INTEGER, name TEXT);"
        result = core.expand_star("SELECT id FROM users", schema_sql=schema)
        # No star to expand → None
        assert result is None

    def test_returns_str_or_none(self, core: types.ModuleType):
        result = core.expand_star("SELECT * FROM t")
        assert result is None or isinstance(result, str)


# ---------------------------------------------------------------------------
# check_source
# ---------------------------------------------------------------------------


class TestCheckSource:
    """Contract: check_source(source, *, dialect, schema_sql, rules) -> list[dict]."""

    def test_detects_stale_rewrite_space(self, core: types.ModuleType):
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = core.check_source(source)
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" in rules

    def test_detects_stale_rewrite_newline(self, core: types.ModuleType):
        source = 'q = "--sql\\nSELECT 1::INTEGER"'
        diags = core.check_source(source)
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" in rules

    def test_clean_source_no_stale_rewrite(self, core: types.ModuleType):
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        diags = core.check_source(source)
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" not in rules

    def test_detects_stale_type(self, core: types.ModuleType):
        source = 'q = SQL[tuple[str]]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = core.check_source(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 1
        assert "tuple[int]" in stale[0]["message"]

    def test_correct_type_no_stale(self, core: types.ModuleType):
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = core.check_source(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 0

    def test_rule_override_ignore(self, core: types.ModuleType):
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = core.check_source(source, rules={"stale-rewrite": "ignore"})
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" not in rules

    def test_diagnostic_has_required_keys(self, core: types.ModuleType):
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = core.check_source(source)
        assert len(diags) >= 1
        d = diags[0]
        assert "rule" in d
        assert "message" in d
        assert "severity" in d
        assert "line" in d

    def test_returns_list(self, core: types.ModuleType):
        assert isinstance(core.check_source("x = 1"), list)

    def test_non_sql_source_returns_empty(self, core: types.ModuleType):
        assert core.check_source("x = 1\ny = 2") == []

    def test_inline_sql_comment_unwrapped(self, core: types.ModuleType):
        """Detects --sql followed by space (query becomes SQL comment)."""
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = core.check_source(source)
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 1
        assert "SQL comment" in inline[0]["message"]

    def test_inline_sql_comment_wrapped(self, core: types.ModuleType):
        """Detects --sql<space> even inside SQL[T](...)."""
        source = 'q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'
        diags = core.check_source(source)
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 1

    def test_no_inline_sql_comment_with_newline_escape(self, core: types.ModuleType):
        """No warning when --sql is followed by \\n (correct format)."""
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        diags = core.check_source(source)
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 0

    def test_inline_sql_comment_disabled(self, core: types.ModuleType):
        """Rule can be disabled via overrides."""
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = core.check_source(source, rules={"inline-sql-comment": "ignore"})
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 0


# ---------------------------------------------------------------------------
# UNTYPED sentinel
# ---------------------------------------------------------------------------

UNTYPED_SOURCE = 'q = "--sql\\nVALUES (1, 2)"'


class TestUntyped:
    """Contract: ``untyped=True`` wraps uninferable queries as ``SQL[UNTYPED](...)``."""

    def test_skipped_by_default(self, core: types.ModuleType):
        assert core.rewrite_source(UNTYPED_SOURCE) == UNTYPED_SOURCE

    @pytest.mark.parametrize(
        ("style", "expected"),
        [
            (
                None,
                "from sqlty import SQL\nfrom sqlty import UNTYPED\n"
                'q = SQL[UNTYPED]("--sql\\nVALUES (1, 2)")',
            ),
            (
                "from sqlty import SQL as Q",
                "from sqlty import SQL as Q\nfrom sqlty import UNTYPED\n"
                'q = Q[UNTYPED]("--sql\\nVALUES (1, 2)")',
            ),
            (
                "import sqlty",
                'import sqlty\nq = sqlty.SQL[sqlty.UNTYPED]("--sql\\nVALUES (1, 2)")',
            ),
            (
                "import sqlty as sy",
                'import sqlty as sy\nq = sy.SQL[sy.UNTYPED]("--sql\\nVALUES (1, 2)")',
            ),
        ],
    )
    def test_rewrite_strip_roundtrip(
        self, core: types.ModuleType, style: str | None, expected: str
    ):
        rewritten = core.rewrite_source(UNTYPED_SOURCE, import_style=style, untyped=True)
        assert rewritten == expected
        assert core.rewrite_source(rewritten, import_style=style, untyped=True) == rewritten
        assert core.strip_source(rewritten, import_style=style) == UNTYPED_SOURCE

    @pytest.mark.parametrize(
        "query",
        ["VALUES (1, 2)", "SELECT FROM users", "not valid sql at all {{{}}"],
    )
    def test_wraps_uninferable_rows(self, core: types.ModuleType, query: str):
        source = f'q = "--sql\\n{query}"'
        rewritten = core.rewrite_source(source, untyped=True)
        assert f'q = SQL[UNTYPED]("--sql\\n{query}")' in rewritten
        assert core.strip_source(rewritten) == source

    @pytest.mark.parametrize(
        "source",
        [
            'q = "--sql\\nDELETE FROM users"',
            'q = "--sql\\nINSERT INTO users (id) VALUES (1)"',
            'q = "--sql\\nUPDATE users SET id = 1"',
            'q = "--sql\\nCREATE TABLE users (id INTEGER)"',
            'q = "--sql\\nDROP TABLE users"',
            'q = "--sql\\nBEGIN"',
            'q = "--sql\\nCOMMIT"',
            'q = "--sql\\n"',
            'q = "--sql VALUES (1, 2)"',
            'q = "--sql SELECT FROM users"',
        ],
    )
    def test_skips_statements_without_rows(self, core: types.ModuleType, source: str):
        assert core.rewrite_source(source, untyped=True) == source

    def test_inferable_queries_unaffected(self, core: types.ModuleType):
        source = 'q = "--sql\\nSELECT 1::INTEGER"'
        assert core.rewrite_source(source, untyped=True) == core.rewrite_source(source)

    def test_strip_keeps_import_still_in_use(self, core: types.ModuleType):
        source = 'from sqlty import SQL\nfrom sqlty import UNTYPED\nq = SQL[UNTYPED]("--sql\\nVALUES (1, 2)")\nUNTYPED\n'
        expected = 'from sqlty import UNTYPED\nq = "--sql\\nVALUES (1, 2)"\nUNTYPED\n'
        assert core.strip_source(source) == expected

    def test_check_untyped_query(self, core: types.ModuleType):
        diags = core.check_source('q = SQL[UNTYPED]("--sql\\nVALUES (1, 2)")')
        assert diags == [
            {
                "rule": "untyped-query",
                "message": "SQL[UNTYPED] has no inferable row type; "
                "add casts (e.g. id::INTEGER) or a schema",
                "severity": "warn",
                "line": 1,
            }
        ]

    def test_check_untyped_now_inferable(self, core: types.ModuleType):
        diags = core.check_source(
            'q = SQL[UNTYPED]("--sql\\nSELECT id FROM users")',
            schema_sql="CREATE TABLE users (id INTEGER);",
        )
        assert [d["message"] for d in diags] == [
            "SQL[UNTYPED] can now be SQL[tuple[int]]; strip and rewrite to update"
        ]

    def test_check_untyped_ignored(self, core: types.ModuleType):
        source = 'q = SQL[UNTYPED]("--sql\\nVALUES (1, 2)")'
        assert core.check_source(source, rules={"untyped-query": "ignore"}) == []


# ---------------------------------------------------------------------------
# check_source — user-defined type validation
# ---------------------------------------------------------------------------


class TestCheckSourceUserTypes:
    """Contract: SQL[UserType] validated structurally against SQL columns.

    User-defined type validation (column-count, column-type, column-name) is
    performed by the Python wrapper in ``sqlty.core.check_source()``, which
    delegates base checks to whichever backend is active.  These tests use
    ``sqlty.core.check_source`` directly — the ``core`` fixture only selects
    which backend is loaded, and base-level tests are already covered above.
    """

    @staticmethod
    def _check(source: str, **kw: object) -> list[dict[str, str | int | None]]:
        from sqlty.core import check_source

        return check_source(source, **kw)  # type: ignore[arg-type]

    def test_typeddict_matching(self):
        """TypedDict with matching fields produces no diagnostics."""
        source = """
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_typeddict_count_mismatch(self):
        """TypedDict with wrong field count is detected."""
        source = """
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: str

q = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        count = [d for d in diags if d["rule"] == "column-count-mismatch"]
        assert len(count) == 1
        assert "3 fields" in count[0]["message"]
        assert "2 columns" in count[0]["message"]

    def test_typeddict_type_mismatch(self):
        """TypedDict with wrong field type is detected."""
        source = """
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

q = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"] == "column-type-mismatch"]
        assert len(type_diags) == 1
        assert "User.id is str" in type_diags[0]["message"]
        assert "infers int" in type_diags[0]["message"]

    def test_typeddict_name_mismatch(self):
        """TypedDict with wrong field name is detected."""
        source = """
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    user_id: int
    username: str

q = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        name_diags = [d for d in diags if d["rule"] == "column-name-mismatch"]
        assert len(name_diags) >= 1
        assert "User.user_id" in name_diags[0]["message"]
        assert "'id'" in name_diags[0]["message"]

    def test_namedtuple_matching(self):
        """NamedTuple with matching fields produces no diagnostics."""
        source = """
from typing import NamedTuple
from sqlty import SQL

class UserRow(NamedTuple):
    id: int
    name: str

q = SQL[UserRow]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_namedtuple_type_mismatch(self):
        """NamedTuple field type mismatch is detected."""
        source = """
from typing import NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    id: int
    name: int

q = SQL[Row]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"] == "column-type-mismatch"]
        assert len(type_diags) == 1
        assert "Row.name is int" in type_diags[0]["message"]

    def test_dataclass_matching(self):
        """Dataclass with matching fields produces no diagnostics."""
        source = """
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class Product:
    id: int
    name: str
    price: float

q = SQL[Product]("--sql\\nSELECT id::INTEGER, name::TEXT, price::NUMERIC FROM products")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_dataclass_count_mismatch(self):
        """Dataclass field count mismatch is detected."""
        source = """
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class Product:
    id: int

q = SQL[Product]("--sql\\nSELECT id::INTEGER, name::TEXT FROM products")
"""
        diags = self._check(source)
        count = [d for d in diags if d["rule"] == "column-count-mismatch"]
        assert len(count) == 1

    def test_basemodel_matching(self):
        """Pydantic BaseModel with matching fields produces no diagnostics."""
        source = """
from pydantic import BaseModel
from sqlty import SQL

class UserModel(BaseModel):
    id: int
    name: str

q = SQL[UserModel]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_basemodel_type_mismatch(self):
        """Pydantic BaseModel field type mismatch is detected."""
        source = """
from pydantic import BaseModel
from sqlty import SQL

class UserModel(BaseModel):
    id: int
    name: int

q = SQL[UserModel]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"] == "column-type-mismatch"]
        assert len(type_diags) == 1

    def test_user_type_no_stale_type(self):
        """SQL[UserType] should NOT trigger stale-type (it's structural now)."""
        source = """
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 0, f"stale-type should not fire for user types: {stale}"

    def test_unknown_type_no_stale_type(self):
        """SQL[UnknownType] should NOT trigger stale-type (can't validate)."""
        source = 'q = SQL[SomeExternalType]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = self._check(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 0

    def test_tuple_still_checks_stale_type(self):
        """tuple[...] annotations still use stale-type checking."""
        source = 'q = SQL[tuple[str]]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = self._check(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 1
        assert "tuple[int]" in stale[0]["message"]

    def test_optional_fields_normalized(self):
        """Optional[int] / int | None should match int."""
        source = """
from typing import TypedDict

from sqlty import SQL

class User(TypedDict):
    id: int | None
    name: str

q = SQL[User]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_datetime_field_matching(self):
        """datetime.datetime field matches TIMESTAMP column."""
        source = """
import datetime
from typing import TypedDict
from sqlty import SQL

class Event(TypedDict):
    id: int
    created_at: datetime.datetime

q = SQL[Event]("--sql\\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_uuid_field_matching(self):
        """uuid.UUID field matches UUID column."""
        source = """
import uuid
from typing import TypedDict
from sqlty import SQL

class Token(TypedDict):
    token_id: uuid.UUID

q = SQL[Token]("--sql\\nSELECT token_id::UUID FROM tokens")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_from_datetime_import_matches(self):
        """'from datetime import datetime' matches TIMESTAMP column."""
        source = """
from datetime import datetime
from typing import TypedDict
from sqlty import SQL

class Event(TypedDict):
    id: int
    created_at: datetime

q = SQL[Event]("--sql\\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0, f"Expected no type errors: {type_diags}"

    def test_from_datetime_import_date(self):
        """'from datetime import date' matches DATE column."""
        source = """
from datetime import date
from typing import TypedDict
from sqlty import SQL

class Record(TypedDict):
    event_date: date

q = SQL[Record]("--sql\\nSELECT event_date::DATE FROM records")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_from_uuid_import_matches(self):
        """'from uuid import UUID' matches UUID column."""
        source = """
from uuid import UUID
from typing import TypedDict
from sqlty import SQL

class Token(TypedDict):
    token_id: UUID

q = SQL[Token]("--sql\\nSELECT token_id::UUID FROM tokens")
"""
        diags = self._check(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    @pytest.mark.parametrize("imports", ["", "import not_installed\n"])
    @pytest.mark.parametrize(
        ("annotation", "ok"),
        [
            ("Any", True),
            ("int | Any", True),
            ("Optional[Union[str, Any]]", True),
            ("int | str", True),
            ("float", True),
            ("str | bytes", False),
        ],
    )
    def test_any_and_union_fields(self, imports: str, annotation: str, ok: bool):
        """A field accepts a column whose type fits it, resolved at runtime or from source."""
        source = f"""{imports}from typing import Any, NamedTuple, Optional, Union
from sqlty import SQL

class Row(NamedTuple):
    id: {annotation}

q = SQL[Row]("--sql\\nSELECT id FROM users")
"""
        diags = self._check(source, schema_sql="CREATE TABLE users (id INTEGER PRIMARY KEY);")
        assert [d["message"] for d in diags if d["rule"] == "column-type-mismatch"] == (
            [] if ok else [f"Row.id is {annotation} but query column 'id' infers int"]
        )


# ---------------------------------------------------------------------------
# Performance baselines (lightweight, no external deps)
# ---------------------------------------------------------------------------


class TestPerformanceBaselines:
    """Rough timing baselines — just verify functions complete in reasonable time."""

    @pytest.fixture()
    def many_lines(self) -> str:
        """100-line source with 50 --sql strings."""
        lines = []
        for i in range(50):
            lines.append(f'q{i} = "--sql\\nSELECT id::INTEGER FROM t{i}"')
            lines.append(f"x{i} = {i}")
        return "\n".join(lines)

    def test_infer_type_latency(self, core: types.ModuleType):
        """Single infer_type call should be < 50 ms."""
        import time

        start = time.perf_counter()
        for _ in range(100):
            core.infer_type("SELECT id::INTEGER, name::TEXT FROM users")
        elapsed = time.perf_counter() - start
        per_call_ms = (elapsed / 100) * 1000
        assert per_call_ms < 50, f"infer_type too slow: {per_call_ms:.1f} ms/call"

    def test_rewrite_source_latency(self, core: types.ModuleType, many_lines: str):
        """Rewriting 100 lines (50 queries) should be < 500 ms."""
        import time

        start = time.perf_counter()
        core.rewrite_source(many_lines)
        elapsed_ms = (time.perf_counter() - start) * 1000
        assert elapsed_ms < 500, f"rewrite_source too slow: {elapsed_ms:.0f} ms"

    def test_strip_source_latency(self, core: types.ModuleType, many_lines: str):
        """Stripping 100 lines should be < 200 ms."""
        import time

        rewritten = core.rewrite_source(many_lines)
        start = time.perf_counter()
        core.strip_source(rewritten)
        elapsed_ms = (time.perf_counter() - start) * 1000
        assert elapsed_ms < 200, f"strip_source too slow: {elapsed_ms:.0f} ms"

    def test_check_source_latency(self, core: types.ModuleType, many_lines: str):
        """Checking 100 lines should be < 500 ms."""
        import time

        start = time.perf_counter()
        core.check_source(many_lines)
        elapsed_ms = (time.perf_counter() - start) * 1000
        assert elapsed_ms < 500, f"check_source too slow: {elapsed_ms:.0f} ms"


# ---------------------------------------------------------------------------
# Backend parity
# ---------------------------------------------------------------------------


def _parity_inputs() -> list[pytest.param]:
    """Every SQL and Python block of the mdtests and README, with its test's config."""
    from pathlib import Path

    from tests.mdtest_runner import ASSERTION_RE, parse_mdtest_file

    root = Path(__file__).resolve().parent.parent
    params = []
    for md in [*sorted((root / "tests" / "mdtest").rglob("*.md")), root / "README.md"]:
        for case in parse_mdtest_file(md):
            config = case._get_sqlty_config()
            schema = config.get("schema")
            options = {
                "dialect": config.get("dialect", "postgres"),
                "schema_sql": case.files.get(schema) if schema else None,
                "import_style": config.get("import-style"),
                "untyped": config.get("untyped") == "sentinel",
            }
            sources = [
                "\n".join(
                    line for line in block.splitlines() if not ASSERTION_RE.match(line.strip())
                )
                for block in case.python_blocks
            ]
            name = f"{md.relative_to(root)}::{case.name}"
            params += [
                pytest.param("sql", sql, options, id=f"{name}::sql{i}")
                for i, sql in enumerate(case.sql_blocks)
            ]
            params += [
                pytest.param("python", source, options, id=f"{name}::python{i}")
                for i, source in enumerate(sources)
            ]
    return params


def _outcome(call, backend: types.ModuleType):
    """A call's result on ``backend``, or its exception type and message."""
    try:
        return call(backend)
    except Exception as exc:
        return type(exc).__name__, str(exc)


@pytest.mark.parametrize(("kind", "text", "options"), _parity_inputs())
def test_backend_parity(kind: str, text: str, options: dict):
    """Rust and Python backends agree on every documented example."""
    rs = pytest.importorskip("sqlty._core", reason="Rust backend not compiled")
    import sqlty._core_py as py

    dialect, schema_sql = options["dialect"], options["schema_sql"]
    import_style, untyped = options["import_style"], options["untyped"]
    if kind == "sql":
        calls = {
            "infer_type": lambda m: m.infer_type(text, dialect=dialect, schema_sql=schema_sql),
            "expand_star": lambda m: m.expand_star(text, dialect=dialect, schema_sql=schema_sql),
        }
    else:
        calls = {
            "rewrite_source": lambda m: m.rewrite_source(
                text,
                dialect=dialect,
                schema_sql=schema_sql,
                import_style=import_style,
                untyped=untyped,
            ),
            "strip_source": lambda m: m.strip_source(text, import_style=import_style),
            "check_source": lambda m: m.check_source(
                text, dialect=dialect, schema_sql=schema_sql, import_style=import_style
            ),
        }
    for name, call in calls.items():
        assert _outcome(call, rs) == _outcome(call, py), name


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


class TestParityRegressions:
    """Cases where the backends used to diverge; each must hold on both."""

    @pytest.mark.parametrize(
        ("sql", "expected"),
        [
            ("DELETE FROM t RETURNING id::INTEGER", "tuple[int]"),
            ("WITH x AS (SELECT 1) DELETE FROM t RETURNING id::INTEGER", "tuple[int]"),
            ("WITH x AS (SELECT 1) UPDATE t SET a = 1 RETURNING id::INTEGER", "tuple[int]"),
            (
                "WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x RETURNING id::INTEGER",
                "tuple[int]",
            ),
            (
                "WITH n AS (INSERT INTO t VALUES (1) RETURNING id) SELECT id::INTEGER FROM n",
                "tuple[int]",
            ),
            ("INSERT INTO t SELECT a::INTEGER FROM u", None),
            ("UPDATE t SET a = (SELECT b::INTEGER FROM u)", None),
            ("CREATE TABLE x AS SELECT a::INTEGER FROM t", None),
            ("SELECT a::INTEGER FROM t UNION SELECT b FROM u UNION SELECT c FROM v", "tuple[int]"),
            ("SELECT 1, 1.5, 'a', TRUE, NULL", "tuple[int, float, str, bool, str]"),
            ("SELECT COUNT(*) AS n, SUM(x) AS s FROM t", "tuple[int, float]"),
            (
                "SELECT AVG(x::INTEGER), MIN(x::INTEGER), MAX(d::DATE) FROM t",
                "tuple[float, int, datetime.date]",
            ),
            ("SELECT COALESCE(x::INTEGER, 0), (x::INTEGER) FROM t", "tuple[int, int]"),
            ("SELECT CASE WHEN a THEN 1::INTEGER ELSE 2 END FROM t", "tuple[int]"),
            ("SELECT EXISTS (SELECT 1), x NOT IN (SELECT 1) FROM t", "tuple[bool, bool]"),
            ("SELECT (SELECT y::INTEGER FROM u) FROM t", "tuple[int]"),
            ("SELECT ROW_NUMBER() OVER (), json_build_object('a', 1) FROM t", "tuple[int, dict]"),
            ("SELECT ts::DOUBLE PRECISION, tags::INT[] FROM t", "tuple[float, list]"),
        ],
    )
    def test_infer_type(self, core: types.ModuleType, sql: str, expected: str | None):
        assert core.infer_type(sql) == expected

    _SCHEMA = (
        "CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));"
        "CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);"
    )

    @pytest.mark.parametrize(
        ("sql", "expected"),
        [
            ("SELECT * FROM users", "tuple[int, str, float]"),
            (
                "SELECT o.*, u.name FROM users u JOIN orders o ON TRUE",
                "tuple[int, int, datetime.datetime, str]",
            ),
            (
                "SELECT u.id, placed FROM users u JOIN orders o ON TRUE",
                "tuple[int, datetime.datetime]",
            ),
            ("SELECT * FROM audit_log", "tuple[str]"),
            ("SELECT MAX(placed), AVG(oid) FROM orders", "tuple[datetime.datetime, float]"),
            ("DELETE FROM users RETURNING *", "tuple[int, str, float]"),
            ("DELETE FROM users RETURNING id, name", "tuple[int, str]"),
        ],
    )
    def test_infer_type_with_schema(self, core: types.ModuleType, sql: str, expected: str):
        assert core.infer_type(sql, schema_sql=self._SCHEMA) == expected

    def test_sum_of_unknown_schema_column_raises(self, core: types.ModuleType):
        with pytest.raises(RuntimeError, match="Column 'nope' not found in table 'users'"):
            core.infer_type("SELECT SUM(nope) FROM users", schema_sql=self._SCHEMA)

    @pytest.mark.parametrize(
        ("sql", "expected"),
        [
            ("SELECT * FROM users", "SELECT id::INT, name::TEXT, score::DECIMAL(10, 2) FROM users"),
            (
                "SELECT o.*, 1 AS one FROM orders o",
                "SELECT oid::INT, user_id::INT, placed::TIMESTAMPTZ, 1 AS one FROM orders o",
            ),
            ("SELECT * FROM users, audit_log", None),
        ],
    )
    def test_expand_star(self, core: types.ModuleType, sql: str, expected: str | None):
        assert core.expand_star(sql, schema_sql=self._SCHEMA) == expected

    def test_rewrite_multiline_triple_quoted(self, core: types.ModuleType):
        source = 'def f():\n    return """--sql\n    SELECT id::INTEGER\n    FROM users\n    """\n'
        rewritten = core.rewrite_source(source)
        assert rewritten == (
            "from sqlty import SQL\n"
            'def f():\n    return SQL[tuple[int]]("""--sql\n    SELECT id::INTEGER\n'
            '    FROM users\n    """)\n'
        )
        assert core.strip_source(rewritten) == source

    def test_formatter_split_wrapper(self, core: types.ModuleType):
        wrapped = (
            'from sqlty import SQL\nq = SQL[tuple[int]](\n    "--sql\\nSELECT 1::INTEGER"\n)\n'
        )
        assert core.rewrite_source(wrapped) == wrapped
        assert core.strip_source(wrapped) == 'q = "--sql\\nSELECT 1::INTEGER"\n'

    def test_rewrite_keeps_crlf(self, core: types.ModuleType):
        source = 'q = "--sql\\nSELECT 1::INTEGER"\r\nx = 1\r\n'
        rewritten = core.rewrite_source(source)
        assert rewritten.endswith('("--sql\\nSELECT 1::INTEGER")\r\nx = 1\r\n')
        assert core.strip_source(rewritten) == source

    def test_rewrite_skips_prefixed_strings(self, core: types.ModuleType):
        source = 'a = f"--sql\\nSELECT {x}::INTEGER"\nb = r"--sql\\nSELECT 1::INTEGER"'
        assert core.rewrite_source(source) == source

    def test_check_skips_user_type_annotations(self, core: types.ModuleType):
        source = 'q = SQL[User]("--sql\\nSELECT id::INTEGER FROM users")'
        assert core.check_source(source) == []

    def test_check_skips_uninferable_query(self, core: types.ModuleType):
        source = 'q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")'
        assert [d["rule"] for d in core.check_source(source)] == ["inline-sql-comment"]

    @pytest.mark.parametrize(
        ("schema", "message"),
        [
            (None, "SELECT * used but no schema available"),
            ("CREATE TABLE users (id INT);", "SELECT * used — expand to explicit column list"),
        ],
    )
    def test_check_select_star_message(
        self, core: types.ModuleType, schema: str | None, message: str
    ):
        source = 'q = SQL[tuple[str]]("--sql\\nselect * FROM users")'
        diags = core.check_source(source, schema_sql=schema)
        assert [d["message"] for d in diags if d["rule"].startswith("select-star")] == [message]

"""Conformance tests for the core API contract.

Every function in ``sqlty.core`` must pass these tests identically, whether
backed by ``_core_py`` (pure Python) or a future ``_core`` (Rust via pyo3).

The tests are organised by function and cover:
- Expected return types and values for representative inputs
- Edge-case handling (empty string, invalid SQL, None schema, …)
- Round-trip invariants (rewrite → strip → rewrite is stable)
- Idempotency (rewrite(rewrite(x)) == rewrite(x))
- --sql\\n normalisation in output
"""

from __future__ import annotations

import pytest

from sqlty.core import (
    check_source,
    expand_star,
    infer_type,
    rewrite_source,
    strip_source,
)

# ---------------------------------------------------------------------------
# infer_type
# ---------------------------------------------------------------------------


class TestInferType:
    """Contract: infer_type(sql, *, dialect, schema_sql) -> str | None."""

    def test_single_integer(self):
        assert infer_type("SELECT id::INTEGER FROM t") == "tuple[int]"

    def test_multiple_columns(self):
        assert infer_type("SELECT id::INTEGER, name::TEXT FROM t") == "tuple[int, str]"

    def test_float(self):
        assert infer_type("SELECT price::FLOAT FROM t") == "tuple[float]"

    def test_boolean(self):
        assert infer_type("SELECT active::BOOLEAN FROM t") == "tuple[bool]"

    def test_datetime(self):
        result = infer_type("SELECT ts::TIMESTAMP FROM t")
        assert result is not None
        assert "datetime" in result

    def test_uuid(self):
        result = infer_type("SELECT uid::UUID FROM t")
        assert result is not None
        assert "uuid" in result

    def test_returns_none_for_garbage(self):
        assert infer_type("not valid sql at all {{{}}") is None

    def test_returns_none_for_empty(self):
        assert infer_type("") is None

    def test_dialect_postgres(self):
        assert infer_type("SELECT 1::INTEGER", dialect="postgres") == "tuple[int]"

    def test_dialect_mysql(self):
        result = infer_type("SELECT CAST(1 AS SIGNED)", dialect="mysql")
        assert result is not None

    def test_dialect_sqlite(self):
        result = infer_type("SELECT CAST(1 AS INTEGER)", dialect="sqlite")
        assert result is not None

    def test_schema_aware(self):
        schema = "CREATE TABLE users (id INTEGER, name TEXT);"
        result = infer_type("SELECT id, name FROM users", schema_sql=schema)
        assert result is not None
        assert "int" in result
        assert "str" in result

    def test_schema_none(self):
        # Without schema, uncast columns default to str
        result = infer_type("SELECT id FROM users")
        assert result is not None


# ---------------------------------------------------------------------------
# rewrite_source
# ---------------------------------------------------------------------------


class TestRewriteSource:
    """Contract: rewrite_source(source, *, dialect, schema_sql) -> str."""

    def test_wraps_sql_string(self):
        result = rewrite_source('q = "--sql SELECT 1::INTEGER"')
        assert "SQL[tuple[int]]" in result

    def test_normalises_to_newline(self):
        result = rewrite_source('q = "--sql SELECT 1::INTEGER"')
        assert '"--sql\\nSELECT 1::INTEGER"' in result

    def test_adds_sql_import(self):
        result = rewrite_source('q = "--sql SELECT 1::INTEGER"')
        assert "from sqlty import SQL" in result

    def test_adds_datetime_import(self):
        result = rewrite_source('q = "--sql SELECT ts::TIMESTAMP"')
        assert "import datetime" in result

    def test_adds_uuid_import(self):
        result = rewrite_source('q = "--sql SELECT uid::UUID"')
        assert "import uuid" in result

    def test_non_sql_unchanged(self):
        source = 'x = 42\nname = "hello"'
        assert rewrite_source(source) == source

    def test_already_wrapped_unchanged(self):
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        assert rewrite_source(source) == source

    def test_idempotent(self):
        source = 'q = "--sql SELECT id::INTEGER FROM t"'
        once = rewrite_source(source)
        twice = rewrite_source(once)
        assert once == twice

    def test_single_quotes(self):
        result = rewrite_source("q = '--sql SELECT 1::INTEGER'")
        assert "SQL[tuple[int]]" in result

    def test_preserves_surrounding_code(self):
        source = 'x = 1\nq = "--sql SELECT 1::INTEGER"\ny = 2'
        result = rewrite_source(source)
        assert "x = 1" in result
        assert "y = 2" in result

    def test_multiple_queries(self):
        source = 'a = "--sql SELECT 1::INTEGER"\nb = "--sql SELECT id::TEXT FROM t"\n'
        result = rewrite_source(source)
        assert "SQL[tuple[int]]" in result
        assert "SQL[tuple[str]]" in result

    def test_returns_str(self):
        assert isinstance(rewrite_source("x = 1"), str)

    # --- Corner cases: queries that should NOT be wrapped ---

    def test_invalid_sql_not_wrapped(self):
        source = 'q = "--sql NOT VALID SQL"'
        assert "SQL[" not in rewrite_source(source)

    def test_empty_query_not_wrapped(self):
        source = 'q = "--sql"'
        assert "SQL[" not in rewrite_source(source)

    def test_whitespace_only_not_wrapped(self):
        source = 'q = "--sql   "'
        assert "SQL[" not in rewrite_source(source)

    def test_update_without_returning_not_wrapped(self):
        source = "q = \"--sql UPDATE users SET name = 'x'\""
        assert "SQL[" not in rewrite_source(source)

    def test_insert_without_returning_not_wrapped(self):
        source = """q = "--sql INSERT INTO users (name) VALUES ('x')" """
        assert "SQL[" not in rewrite_source(source)

    def test_delete_without_returning_not_wrapped(self):
        source = 'q = "--sql DELETE FROM users"'
        assert "SQL[" not in rewrite_source(source)

    def test_create_table_not_wrapped(self):
        source = 'q = "--sql CREATE TABLE t (id INT)"'
        assert "SQL[" not in rewrite_source(source)

    def test_drop_table_not_wrapped(self):
        source = 'q = "--sql DROP TABLE users"'
        assert "SQL[" not in rewrite_source(source)

    def test_begin_not_wrapped(self):
        source = 'q = "--sql BEGIN"'
        assert "SQL[" not in rewrite_source(source)

    def test_dml_with_returning_is_wrapped(self):
        source = 'q = "--sql UPDATE users SET x=1 RETURNING id::INTEGER"'
        result = rewrite_source(source)
        assert "SQL[tuple[int]]" in result

    def test_select_without_casts_is_wrapped(self):
        """SELECT without casts still wraps — type defaults to tuple[str, ...]."""
        source = 'q = "--sql SELECT id, name FROM users"'
        result = rewrite_source(source)
        assert "SQL[tuple[str, str]]" in result


# ---------------------------------------------------------------------------
# strip_source
# ---------------------------------------------------------------------------


class TestStripSource:
    """Contract: strip_source(source) -> str."""

    def test_removes_wrapper(self):
        source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        result = strip_source(source)
        assert "SQL[" not in result
        assert '"--sql\\nSELECT 1::INTEGER"' in result

    def test_removes_sql_import(self):
        source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        result = strip_source(source)
        assert "from sqlty import SQL" not in result

    def test_preserves_manual_annotation(self):
        source = 'q = SQL[int]("SELECT 1")'
        assert strip_source(source) == source

    def test_preserves_non_sql(self):
        source = "x = 42"
        assert strip_source(source) == source

    def test_idempotent(self):
        source = 'q = "--sql\\nSELECT 1::INTEGER"'
        once = strip_source(source)
        twice = strip_source(once)
        assert once == twice

    def test_handles_space_format(self):
        # Legacy space format is also strippable
        source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'
        result = strip_source(source)
        assert "SQL[" not in result

    def test_returns_str(self):
        assert isinstance(strip_source("x = 1"), str)


# ---------------------------------------------------------------------------
# Round-trip: rewrite ↔ strip
# ---------------------------------------------------------------------------


class TestRoundTrip:
    """Contract: strip(rewrite(x)) produces a stable string; second cycle is identity."""

    def test_rewrite_then_strip_is_stable(self):
        original = 'q = "--sql SELECT id::INTEGER, name::TEXT FROM t"'
        rewritten = rewrite_source(original)
        stripped = strip_source(rewritten)
        # Second cycle must be identity
        re_rewritten = rewrite_source(stripped)
        re_stripped = strip_source(re_rewritten)
        assert re_stripped == stripped

    def test_strip_then_rewrite_is_stable(self):
        wrapped = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        stripped = strip_source(wrapped)
        rewritten = rewrite_source(stripped)
        re_stripped = strip_source(rewritten)
        assert re_stripped == stripped


# ---------------------------------------------------------------------------
# expand_star
# ---------------------------------------------------------------------------


class TestExpandStar:
    """Contract: expand_star(sql, *, dialect, schema_sql) -> str | None."""

    def test_expands_with_schema(self):
        schema = "CREATE TABLE users (id INTEGER, name TEXT);"
        result = expand_star("SELECT * FROM users", schema_sql=schema)
        assert result is not None
        assert "id" in result
        assert "name" in result
        assert "*" not in result

    def test_returns_none_without_schema(self):
        assert expand_star("SELECT * FROM users") is None

    def test_non_star_returns_none(self):
        schema = "CREATE TABLE users (id INTEGER, name TEXT);"
        result = expand_star("SELECT id FROM users", schema_sql=schema)
        # No star to expand → None
        assert result is None

    def test_returns_str_or_none(self):
        result = expand_star("SELECT * FROM t")
        assert result is None or isinstance(result, str)


# ---------------------------------------------------------------------------
# check_source
# ---------------------------------------------------------------------------


class TestCheckSource:
    """Contract: check_source(source, *, dialect, schema_sql, rules) -> list[dict]."""

    def test_detects_stale_rewrite_space(self):
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = check_source(source)
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" in rules

    def test_detects_stale_rewrite_newline(self):
        source = 'q = "--sql\\nSELECT 1::INTEGER"'
        diags = check_source(source)
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" in rules

    def test_clean_source_no_stale_rewrite(self):
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        diags = check_source(source)
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" not in rules

    def test_detects_stale_type(self):
        source = 'q = SQL[tuple[str]]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = check_source(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 1
        assert "tuple[int]" in stale[0]["message"]

    def test_correct_type_no_stale(self):
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = check_source(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 0

    def test_rule_override_ignore(self):
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = check_source(source, rules={"stale-rewrite": "ignore"})
        rules = [d["rule"] for d in diags]
        assert "stale-rewrite" not in rules

    def test_diagnostic_has_required_keys(self):
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = check_source(source)
        assert len(diags) >= 1
        d = diags[0]
        assert "rule" in d
        assert "message" in d
        assert "severity" in d
        assert "line" in d

    def test_returns_list(self):
        assert isinstance(check_source("x = 1"), list)

    def test_non_sql_source_returns_empty(self):
        assert check_source("x = 1\ny = 2") == []

    def test_inline_sql_comment_unwrapped(self):
        """Detects --sql followed by space (query becomes SQL comment)."""
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = check_source(source)
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 1
        assert "SQL comment" in inline[0]["message"]

    def test_inline_sql_comment_wrapped(self):
        """Detects --sql<space> even inside SQL[T](...)."""
        source = 'q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'
        diags = check_source(source)
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 1

    def test_no_inline_sql_comment_with_newline_escape(self):
        """No warning when --sql is followed by \\n (correct format)."""
        source = 'q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
        diags = check_source(source)
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 0

    def test_inline_sql_comment_disabled(self):
        """Rule can be disabled via overrides."""
        source = 'q = "--sql SELECT 1::INTEGER"'
        diags = check_source(source, rules={"inline-sql-comment": "ignore"})
        inline = [d for d in diags if d["rule"] == "inline-sql-comment"]
        assert len(inline) == 0


# ---------------------------------------------------------------------------
# check_source — user-defined type validation
# ---------------------------------------------------------------------------


class TestCheckSourceUserTypes:
    """Contract: SQL[UserType] validated structurally against SQL columns."""

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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 0, f"stale-type should not fire for user types: {stale}"

    def test_unknown_type_no_stale_type(self):
        """SQL[UnknownType] should NOT trigger stale-type (can't validate)."""
        source = 'q = SQL[SomeExternalType]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = check_source(source)
        stale = [d for d in diags if d["rule"] == "stale-type"]
        assert len(stale) == 0

    def test_tuple_still_checks_stale_type(self):
        """tuple[...] annotations still use stale-type checking."""
        source = 'q = SQL[tuple[str]]("--sql\\nSELECT id::INTEGER FROM t")'
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0

    def test_from_datetime_import_matches(self):
        """'from datetime import datetime' matches TIMESTAMP column.

        This is a key advantage of runtime type checking over string
        comparison: AST would see 'datetime' (bare name) which doesn't
        match sqlty's 'datetime.datetime', but runtime resolution gives
        us the actual datetime.datetime class.
        """
        source = """
from datetime import datetime
from typing import TypedDict
from sqlty import SQL

class Event(TypedDict):
    id: int
    created_at: datetime

q = SQL[Event]("--sql\\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
"""
        diags = check_source(source)
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
        diags = check_source(source)
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
        diags = check_source(source)
        type_diags = [d for d in diags if d["rule"].startswith("column-")]
        assert len(type_diags) == 0


# ---------------------------------------------------------------------------
# Performance baselines (lightweight, no external deps)
# ---------------------------------------------------------------------------


class TestPerformanceBaselines:
    """Rough timing baselines for the Python implementation.

    These aren't strict benchmarks — they just verify the functions
    complete in a reasonable time and give a ballpark for comparison
    when a Rust ``_core`` is introduced.
    """

    @pytest.fixture()
    def many_lines(self) -> str:
        """100-line source with 50 --sql strings."""
        lines = []
        for i in range(50):
            lines.append(f'q{i} = "--sql SELECT id::INTEGER FROM t{i}"')
            lines.append(f"x{i} = {i}")
        return "\n".join(lines)

    def test_infer_type_latency(self):
        """Single infer_type call should be < 50 ms."""
        import time

        start = time.perf_counter()
        for _ in range(100):
            infer_type("SELECT id::INTEGER, name::TEXT FROM users")
        elapsed = time.perf_counter() - start
        per_call_ms = (elapsed / 100) * 1000
        # Just record — don't fail unless absurdly slow
        assert per_call_ms < 50, f"infer_type too slow: {per_call_ms:.1f} ms/call"

    def test_rewrite_source_latency(self, many_lines: str):
        """Rewriting 100 lines (50 queries) should be < 500 ms."""
        import time

        start = time.perf_counter()
        rewrite_source(many_lines)
        elapsed_ms = (time.perf_counter() - start) * 1000
        assert elapsed_ms < 500, f"rewrite_source too slow: {elapsed_ms:.0f} ms"

    def test_strip_source_latency(self, many_lines: str):
        """Stripping 100 lines should be < 200 ms."""
        import time

        rewritten = rewrite_source(many_lines)
        start = time.perf_counter()
        strip_source(rewritten)
        elapsed_ms = (time.perf_counter() - start) * 1000
        assert elapsed_ms < 200, f"strip_source too slow: {elapsed_ms:.0f} ms"

    def test_check_source_latency(self, many_lines: str):
        """Checking 100 lines should be < 500 ms."""
        import time

        start = time.perf_counter()
        check_source(many_lines)
        elapsed_ms = (time.perf_counter() - start) * 1000
        assert elapsed_ms < 500, f"check_source too slow: {elapsed_ms:.0f} ms"

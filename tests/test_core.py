"""Tests for the core API — the Rust/Python boundary.

Verifies that the public `sqlty.core` module:
1. Imports successfully (falls back to _core_py since no Rust extension)
2. All functions return correct types
3. Round-trip: rewrite_source -> strip_source recovers original
4. check_source detects stale rewrites and stale types
"""

from __future__ import annotations


def test_core_imports():
    """The core module should import all public functions."""
    from sqlty.core import (
        check_source,
        expand_star,
        infer_type,
        rewrite_source,
        strip_source,
    )

    assert callable(infer_type)
    assert callable(rewrite_source)
    assert callable(strip_source)
    assert callable(expand_star)
    assert callable(check_source)


def test_infer_type_basic():
    """infer_type returns a Python type string."""
    from sqlty.core import infer_type

    result = infer_type("SELECT id::INTEGER, name::TEXT FROM users")
    assert result == "tuple[int, str]"


def test_infer_type_none():
    """infer_type returns None for non-parseable SQL."""
    from sqlty.core import infer_type

    result = infer_type("not valid sql at all {{{}}")
    assert result is None


def test_infer_type_dialect():
    """infer_type respects dialect parameter."""
    from sqlty.core import infer_type

    result = infer_type("SELECT id::INTEGER FROM users", dialect="postgres")
    assert result == "tuple[int]"


def test_rewrite_source_basic():
    """rewrite_source wraps --sql strings in SQL[T](...)."""
    from sqlty.core import rewrite_source

    source = 'q = "--sql SELECT id::INTEGER FROM users"'
    result = rewrite_source(source)
    assert "SQL[tuple[int]]" in result
    assert "from sqlty import SQL" in result


def test_rewrite_source_no_change():
    """rewrite_source returns unchanged source when no --sql strings."""
    from sqlty.core import rewrite_source

    source = 'q = "SELECT 1"'
    result = rewrite_source(source)
    assert result == source


def test_strip_source_basic():
    """strip_source removes SQL[T](...) wrappers."""
    from sqlty.core import strip_source

    source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM users")'
    result = strip_source(source)
    assert "SQL[" not in result
    assert '"--sql\\nSELECT id::INTEGER FROM users"' in result


def test_round_trip():
    """rewrite -> strip -> rewrite -> strip is idempotent."""
    from sqlty.core import rewrite_source, strip_source

    original = 'q = "--sql SELECT id::INTEGER, name::TEXT FROM users"'
    rewritten = rewrite_source(original)
    # Rewrite normalizes --sql<space> to --sql\n
    assert '"--sql\\nSELECT' in rewritten
    stripped = strip_source(rewritten)
    # Second cycle is stable (idempotent)
    re_rewritten = rewrite_source(stripped)
    re_stripped = strip_source(re_rewritten)
    assert re_stripped.strip() == stripped.strip()


def test_expand_star_with_schema():
    """expand_star replaces SELECT * with explicit columns from schema."""
    from sqlty.core import expand_star

    schema_sql = "CREATE TABLE users (id INTEGER, name TEXT);"
    result = expand_star(
        "SELECT * FROM users", dialect="postgres", schema_sql=schema_sql
    )
    assert result is not None
    assert "id" in result
    assert "name" in result
    assert "*" not in result


def test_expand_star_no_schema():
    """expand_star returns None without schema."""
    from sqlty.core import expand_star

    result = expand_star("SELECT * FROM users")
    assert result is None


def test_strip_preserves_manual_annotations():
    """strip_source does NOT strip manually written SQL[T]() without --sql prefix."""
    from sqlty.core import strip_source

    source = 'q = SQL[int]("SELECT 1")'
    result = strip_source(source)
    assert result == source  # unchanged — no --sql prefix


def test_strip_newline_format():
    """strip_source strips --sql\\n format strings."""
    from sqlty.core import strip_source

    source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'
    result = strip_source(source)
    assert "SQL[" not in result
    assert '"--sql\\nSELECT 1::INTEGER"' in result


def test_check_source_stale_rewrite():
    """check_source detects unwrapped --sql strings."""
    from sqlty.core import check_source

    source = 'q = "--sql SELECT id::INTEGER FROM users"'
    diagnostics = check_source(source)
    assert len(diagnostics) >= 1
    assert diagnostics[0]["rule"] == "stale-rewrite"


def test_check_source_clean():
    """check_source returns empty for clean wrapped source."""
    from sqlty.core import check_source

    source = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM users")'
    diagnostics = check_source(source)
    # No stale-rewrite (it's wrapped), no stale-type (type is correct)
    stale_rewrites = [d for d in diagnostics if d["rule"] == "stale-rewrite"]
    assert len(stale_rewrites) == 0


def test_check_source_stale_type():
    """check_source detects stale type annotations."""
    from sqlty.core import check_source

    # The annotation says tuple[str] but the query infers tuple[int]
    source = 'q = SQL[tuple[str]]("--sql\\nSELECT id::INTEGER FROM users")'
    diagnostics = check_source(source)
    stale_types = [d for d in diagnostics if d["rule"] == "stale-type"]
    assert len(stale_types) == 1
    assert "tuple[int]" in stale_types[0]["message"]


def test_check_source_rule_override():
    """check_source respects rule severity overrides."""
    from sqlty.core import check_source

    source = 'q = "--sql SELECT id::INTEGER FROM users"'
    # Disable stale-rewrite
    diagnostics = check_source(source, rules={"stale-rewrite": "ignore"})
    stale_rewrites = [d for d in diagnostics if d["rule"] == "stale-rewrite"]
    assert len(stale_rewrites) == 0

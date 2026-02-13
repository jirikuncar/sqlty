"""Core computation functions — the Rust/Python boundary.

This module defines the pure-computation functions that form the FFI boundary
between Python and a future Rust extension. Each function takes simple types
(str, dict, list) and returns simple types — no Path objects, no file I/O.

Import pattern (used by all callers):

    try:
        from sqlty._core import infer_type, rewrite_source, strip_source, check_source
    except ImportError:
        from sqlty._core_py import infer_type, rewrite_source, strip_source, check_source

When the Rust extension is available (compiled as `sqlty._core` via maturin/pyo3),
it replaces these Python implementations transparently.

Design constraints for Rust portability:
- All inputs/outputs are primitive types (str, list[str], dict[str, str], None)
- No Path objects, no file I/O, no imports of external packages in signatures
- Schema is passed as a SQL string (CREATE TABLE ...), not a file path
- Config is passed as a dict, not a parsed TOML object
"""

from __future__ import annotations

import re

from sqlty._checker import Checker
from sqlty._lint import RuleConfig
from sqlty._rewriter import SQLRewriter
from sqlty._sql_analyzer import SQLAnalyzer


def infer_type(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> str | None:
    """Infer the Python return type for a SQL query.

    Args:
        sql: SQL query string (e.g., "SELECT id::INTEGER, name::TEXT FROM users")
        dialect: SQL dialect (postgres, mysql, sqlite)
        schema_sql: Optional CREATE TABLE statements for schema-aware inference

    Returns:
        Python type string (e.g., "tuple[int, str]") or None if inference fails
    """
    analyzer = _make_analyzer(dialect, schema_sql)
    return analyzer.generate_return_type(sql)


def rewrite_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> str:
    """Rewrite Python source: "--sql ..." -> SQL[T]("--sql ...").

    Adds necessary imports (SQL, datetime, uuid) at the top.

    Args:
        source: Python source code
        dialect: SQL dialect
        schema_sql: Optional CREATE TABLE statements

    Returns:
        Rewritten source code
    """
    rewriter = _make_rewriter(dialect, schema_sql)
    return rewriter.rewrite(source)


def strip_source(source: str) -> str:
    """Strip Python source: SQL[T]("--sql ...") -> "--sql ...".

    Removes unused imports added by rewrite.

    Args:
        source: Python source code with SQL[T](...) wrappers

    Returns:
        Stripped source code
    """
    rewriter = SQLRewriter()
    return rewriter.strip(source)


def expand_star(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> str | None:
    """Expand SELECT * into explicit column list.

    Args:
        sql: SQL query containing SELECT *
        dialect: SQL dialect
        schema_sql: CREATE TABLE statements (required for expansion)

    Returns:
        SQL with expanded columns, or None if expansion is not possible
    """
    analyzer = _make_analyzer(dialect, schema_sql)
    return analyzer.expand_star(sql)


def check_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    rules: dict[str, str] | None = None,
) -> list[dict[str, str | int | None]]:
    """Check Python source for SQL type issues.

    Scans source for SQL[T]("--sql ...") patterns and validates:
    - SELECT * usage (select-star, select-star-no-schema)
    - Stale rewrites (stale-rewrite) — --sql strings not yet wrapped
    - Stale types (stale-type) — SQL[T] doesn't match current inference
    - User-defined types (column-count-mismatch, column-type-mismatch,
      column-name-mismatch) — SQL[UserType] fields vs query columns

    When the annotated type is a user-defined type (TypedDict, NamedTuple,
    dataclass, Pydantic BaseModel) found in the same source file, the checker
    validates the type's fields against the SQL query columns structurally
    instead of comparing against the inferred tuple type string.

    Args:
        source: Python source code
        dialect: SQL dialect
        schema_sql: Optional CREATE TABLE statements
        rules: Rule severity overrides (e.g., {"select-star": "ignore"})

    Returns:
        List of diagnostic dicts with keys: rule, message, severity, line
    """
    analyzer = _make_analyzer(dialect, schema_sql)
    rule_config = RuleConfig(overrides=rules)
    checker = Checker(analyzer=analyzer, rule_config=rule_config)
    rewriter = _make_rewriter(dialect, schema_sql)
    has_schema = schema_sql is not None

    diagnostics: list[dict[str, str | int | None]] = []

    for i, line in enumerate(source.splitlines(), 1):
        line_stripped = line.strip()

        # Check for unwrapped --sql strings (stale-rewrite)
        if _is_unwrapped_sql_string(line_stripped):
            if rule_config.is_enabled("stale-rewrite"):
                diagnostics.append(
                    {
                        "rule": "stale-rewrite",
                        "message": "Source has --sql strings that aren't wrapped in SQL[T]",
                        "severity": rule_config.severity("stale-rewrite").value,
                        "line": i,
                    }
                )
            # Also check for inline-sql-comment on unwrapped strings
            if rule_config.is_enabled("inline-sql-comment") and _has_inline_sql_comment(
                line_stripped
            ):
                diagnostics.append(
                    {
                        "rule": "inline-sql-comment",
                        "message": '"--sql SELECT ..." treats the query as a SQL comment; use "--sql\\nSELECT ..." or run sqlty rewrite',
                        "severity": rule_config.severity("inline-sql-comment").value,
                        "line": i,
                    }
                )
            continue

        # Check for SQL[T](...) wrappers
        sql_content = _extract_sql_from_wrapper(line_stripped)
        if sql_content is None:
            continue

        # Check inline-sql-comment on wrapped strings
        if rule_config.is_enabled("inline-sql-comment") and sql_content.startswith("--sql "):
            diagnostics.append(
                {
                    "rule": "inline-sql-comment",
                    "message": '"--sql SELECT ..." treats the query as a SQL comment; use "--sql\\nSELECT ..." or run sqlty rewrite',
                    "severity": rule_config.severity("inline-sql-comment").value,
                    "line": i,
                }
            )

        # Check SELECT *
        star_diag = checker.check_select_star(sql_content, has_schema=has_schema)
        if star_diag:
            diagnostics.append(
                {
                    "rule": star_diag.rule,
                    "message": star_diag.message,
                    "severity": star_diag.severity.value,
                    "line": i,
                }
            )

        # Check tuple type annotations (stale-type)
        # User-defined types (TypedDict, NamedTuple, etc.) are handled by
        # core.py's wrapper using runtime type resolution.
        annotated_type = _extract_type_from_wrapper(line_stripped)
        if annotated_type and annotated_type.startswith("tuple["):
            if rule_config.is_enabled("stale-type"):
                current_type = rewriter._infer_type(sql_content)
                if current_type != annotated_type:
                    diagnostics.append(
                        {
                            "rule": "stale-type",
                            "message": f"SQL[{annotated_type}] should be SQL[{current_type}]",
                            "severity": rule_config.severity("stale-type").value,
                            "line": i,
                        }
                    )

    return diagnostics


# --- Internal helpers ---


def _strip_sql_prefix(sql_content: str) -> str:
    """Strip --sql prefix from SQL content to get the actual SQL query."""
    if sql_content.startswith("--sql\\n"):
        return sql_content[7:].strip()
    if sql_content.startswith("--sql "):
        return sql_content[6:].strip()
    if sql_content.startswith("--sql\n"):
        return sql_content[6:].strip()
    if sql_content.startswith("--sql"):
        return sql_content[5:].strip()
    return sql_content


def _make_analyzer(dialect: str, schema_sql: str | None) -> SQLAnalyzer:
    """Create an SQLAnalyzer, optionally loading schema from SQL string."""
    import tempfile
    from pathlib import Path

    if schema_sql:
        tmp = Path(tempfile.mktemp(suffix=".sql"))
        tmp.write_text(schema_sql)
        try:
            return SQLAnalyzer(dialect=dialect, schema_path=tmp)
        finally:
            tmp.unlink(missing_ok=True)
    return SQLAnalyzer(dialect=dialect)


def _make_rewriter(dialect: str, schema_sql: str | None) -> SQLRewriter:
    """Create an SQLRewriter, optionally with schema."""
    import tempfile

    if schema_sql:
        tmp_path = tempfile.mktemp(suffix=".sql")
        with open(tmp_path, "w") as f:
            f.write(schema_sql)
        return SQLRewriter(dialect=dialect, schema_path=tmp_path)
    return SQLRewriter(dialect=dialect)


def _has_inline_sql_comment(line: str) -> bool:
    """Check if a line has --sql followed by space (not \\n).

    ``"--sql SELECT ..."`` is problematic because ``--sql SELECT ...`` is a
    SQL line comment, making the entire query empty.  The correct format is
    ``"--sql\\nSELECT ..."`` where the actual SQL starts on the next line.
    """
    return bool(re.search(r"""["']--sql [^\\]""", line))


def _is_unwrapped_sql_string(line: str) -> bool:
    """Check if a line contains an unwrapped --sql string."""
    # Match: variable = "--sql ..." or variable = '--sql ...'
    # Handles both --sql<space> and --sql\n (source-text escape) formats.
    # But NOT already wrapped in SQL[...]
    if "SQL[" in line:
        return False
    return bool(re.search(r"""["']--sql(\s|\\n)""", line))


def _extract_sql_from_wrapper(line: str) -> str | None:
    """Extract SQL content from SQL[T]("--sql ...") pattern."""
    match = re.search(r'SQL\[.*?\]\(\s*["\'](.+?)["\']\s*\)', line, re.DOTALL)
    if match:
        content = match.group(1)
        if content.startswith("--sql"):
            return content
    return None


def _extract_type_from_wrapper(line: str) -> str | None:
    """Extract the type annotation from SQL[T](...) pattern."""
    match = re.search(r"SQL\[(.+?)\]\(", line)
    if match:
        return match.group(1)
    return None

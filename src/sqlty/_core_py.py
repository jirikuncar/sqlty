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
from sqlty._rewriter import SKIPPED_STRING_MESSAGES, SQLRewriter, _header_end, skipped_string_rule
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
    import_style: str | None = None,
    untyped: bool = False,
) -> str:
    """Rewrite Python source: "--sql ..." -> SQL[T]("--sql ...").

    Adds necessary imports (SQL, datetime, uuid) at the top.

    Args:
        source: Python source code
        dialect: SQL dialect
        schema_sql: Optional CREATE TABLE statements
        import_style: Import style (e.g. ``"from sqlty import SQL as Q"``)
        untyped: Wrap row-returning queries of unknown row type as ``SQL[UNTYPED](...)``

    Returns:
        Rewritten source code
    """
    rewriter = _make_rewriter(dialect, schema_sql, import_style=import_style, untyped=untyped)
    return rewriter.rewrite(source).source


def strip_source(source: str, *, import_style: str | None = None) -> str:
    """Strip Python source: SQL[T]("--sql ...") -> "--sql ...".

    Removes unused imports added by rewrite.

    Args:
        source: Python source code with SQL[T](...) wrappers
        import_style: Import style (e.g. ``"from sqlty import SQL as Q"``)

    Returns:
        Stripped source code
    """
    rewriter = SQLRewriter(import_style=import_style)
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
    import_style: str | None = None,
) -> list[dict[str, str | int | None]]:
    """Check Python source for SQL type issues.

    Scans source for SQL[T]("--sql ...") patterns and validates:
    - SELECT * usage (select-star, select-star-no-schema)
    - Stale rewrites (stale-rewrite) — --sql strings not yet wrapped
    - Stale types (stale-type) — SQL[T] doesn't match current inference
    - Unknown row types (untyped-query) — SQL[UNTYPED] left by ``rewrite --untyped sentinel``
    - Interpolated strings (fstring-skipped, tstring-skipped) — f"--sql ..." is never rewritten

    User-defined types (``SQL[UserType]``) are skipped here; ``sqlty.core.check_source``
    validates them on top of either backend.

    Args:
        source: Python source code
        dialect: SQL dialect
        schema_sql: Optional CREATE TABLE statements
        rules: Rule severity overrides (e.g., {"select-star": "ignore"})
        import_style: Import style (e.g. ``"from sqlty import SQL as Q"``)

    Returns:
        List of diagnostic dicts with keys: rule, message, severity, line
    """
    from sqlty._rewriter import ImportStyle

    analyzer = _make_analyzer(dialect, schema_sql)
    rule_config = RuleConfig(overrides=rules)
    checker = Checker(analyzer=analyzer, rule_config=rule_config)
    rewriter = _make_rewriter(dialect, schema_sql, import_style=import_style)
    has_schema = schema_sql is not None

    istyle = ImportStyle.parse(import_style) if import_style else ImportStyle.default()
    wrapper_name = istyle.wrapper_name
    untyped_name = istyle.untyped_name

    diagnostics: list[dict[str, str | int | None]] = []
    wrappable = _wrappable_lines(source, rewriter)

    for i, line_stripped in _logical_lines(source, wrapper_name):
        if skipped := skipped_string_rule(line_stripped):
            if rule_config.is_enabled(skipped):
                diagnostics.append(
                    {
                        "rule": skipped,
                        "message": SKIPPED_STRING_MESSAGES[skipped],
                        "severity": rule_config.severity(skipped).value,
                        "line": i,
                    }
                )
            continue

        # Check for unwrapped --sql strings (stale-rewrite)
        if _is_unwrapped_sql_string(line_stripped, wrapper_name):
            if i in wrappable and rule_config.is_enabled("stale-rewrite"):
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

        # Check for wrapper patterns
        sql_content = _extract_sql_from_wrapper(line_stripped, wrapper_name)
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
        annotated_type = _extract_type_from_wrapper(line_stripped, wrapper_name)
        if annotated_type == untyped_name and rule_config.is_enabled("untyped-query"):
            current_type = rewriter._infer_type(sql_content)
            untyped = f"{wrapper_name}[{untyped_name}]"
            message = (
                f"{untyped} has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
                if current_type == "Any"
                else f"{untyped} can now be {wrapper_name}[{current_type}]; run sqlty check --fix"
            )
            diagnostics.append(
                {
                    "rule": "untyped-query",
                    "message": message,
                    "severity": rule_config.severity("untyped-query").value,
                    "line": i,
                }
            )
        if annotated_type and annotated_type.startswith("tuple["):
            if rule_config.is_enabled("stale-type"):
                current_type = rewriter._infer_type(sql_content)
                if current_type not in ("Any", annotated_type):
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
        fd, tmp_name = tempfile.mkstemp(suffix=".sql")
        tmp = Path(tmp_name)
        try:
            tmp.write_text(schema_sql)
            return SQLAnalyzer(dialect=dialect, schema_path=tmp)
        finally:
            import os

            os.close(fd)
            tmp.unlink(missing_ok=True)
    return SQLAnalyzer(dialect=dialect)


def _make_rewriter(
    dialect: str,
    schema_sql: str | None,
    *,
    import_style: str | None = None,
    untyped: bool = False,
) -> SQLRewriter:
    """Create an SQLRewriter, optionally with schema."""
    import tempfile

    schema_path = None
    if schema_sql:
        fd, schema_path = tempfile.mkstemp(suffix=".sql")
        with open(fd, "w") as f:
            f.write(schema_sql)
    return SQLRewriter(
        dialect=dialect, schema_path=schema_path, import_style=import_style, untyped=untyped
    )


def _has_inline_sql_comment(line: str) -> bool:
    """Check if a line has --sql followed by space (not \\n).

    ``"--sql SELECT ..."`` is problematic because ``--sql SELECT ...`` is a
    SQL line comment, making the entire query empty.  The correct format is
    ``"--sql\\nSELECT ..."`` where the actual SQL starts on the next line.
    """
    return bool(re.search(r"""["']--sql [^\\]""", line))


def _wrappable_lines(source: str, rewriter: SQLRewriter) -> set[int]:
    """Lines holding a string that ``rewrite`` would wrap.

    Rewrite only inserts imports at the header and wraps strings in place,
    so each other line keeps its position, shifted past the imports.

    Example:
        >>> _wrappable_lines('a = "--sql\\\\nSELECT 1::INTEGER"\\nb = "--sql\\\\nDROP TABLE t"', SQLRewriter())
        {1}
    """
    original = source.splitlines()
    rewritten = rewriter.rewrite(source).source.splitlines()
    added = len(rewritten) - len(original)
    head = source[: _header_end(source)].count("\n")
    return {
        i + 1 for i, line in enumerate(original) if rewritten[i if i < head else i + added] != line
    }


def _logical_lines(source: str, wrapper_name: str) -> list[tuple[int, str]]:
    """Stripped lines, with a wrapper split by a formatter joined onto its first line.

    Example:
        >>> _logical_lines('q = SQL[tuple[int]](\\n    "--sql\\\\nSELECT 1"\\n)', "SQL")
        [(1, 'q = SQL[tuple[int]]("--sql\\\\nSELECT 1")')]
    """
    opener = re.compile(rf"{re.escape(wrapper_name)}\[.*\]\($")
    lines = [line.strip() for line in source.splitlines()]
    result: list[tuple[int, str]] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if opener.search(line) and i + 2 < len(lines) and lines[i + 2].startswith(")"):
            result.append((i + 1, line + lines[i + 1].rstrip(",") + lines[i + 2]))
            i += 3
            continue
        result.append((i + 1, line))
        i += 1
    return result


def _is_unwrapped_sql_string(line: str, wrapper_name: str = "SQL") -> bool:
    """Check if a line contains an unwrapped --sql string."""
    # Match: variable = "--sql ..." or variable = '--sql ...'
    # Handles both --sql<space> and --sql\n (source-text escape) formats.
    # But NOT already wrapped in {wrapper_name}[...]
    if f"{wrapper_name}[" in line:
        return False
    return bool(re.search(r"""["']--sql(\s|\\n)""", line))


def _extract_sql_from_wrapper(line: str, wrapper_name: str = "SQL") -> str | None:
    """Extract SQL content from {wrapper_name}[T]("--sql ...") pattern."""
    escaped = re.escape(wrapper_name)
    match = re.search(rf'{escaped}\[.*?\]\(\s*["\'](.+?)["\']\s*\)', line, re.DOTALL)
    if match:
        content = match.group(1)
        if content.startswith("--sql"):
            return content
    return None


def _extract_type_from_wrapper(line: str, wrapper_name: str = "SQL") -> str | None:
    """Extract the type annotation from {wrapper_name}[T](...) pattern."""
    escaped = re.escape(wrapper_name)
    match = re.search(rf"{escaped}\[(.+?)\]\(", line)
    if match:
        return match.group(1)
    return None

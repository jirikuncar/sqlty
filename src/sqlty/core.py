"""Public core API — import-switching facade.

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

Currently uses ``sqlty._core_py`` (pure Python). When a Rust extension is
compiled via maturin/pyo3, it will be installed as ``sqlty._core`` and
this module will import from it instead. Both implementations must satisfy
the same API contract (see ``tests/test_core_conformance.py``).

``check_source`` is wrapped to add Python-side user-defined type checking
(TypedDict, NamedTuple, dataclass, Pydantic) on top of whichever backend
is active, since type extraction requires Python's ``ast`` module.
"""

from __future__ import annotations

try:
    from sqlty._core import (  # type: ignore[import-not-found]
        check_source as _backend_check_source,
        expand_star,
        infer_type,
        rewrite_source,
        strip_source,
    )
except ImportError:
    from sqlty._core_py import (
        check_source as _backend_check_source,
        expand_star,
        infer_type,
        rewrite_source,
        strip_source,
    )


def check_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    rules: dict[str, str] | None = None,
) -> list[dict[str, str | int | None]]:
    """Check Python source for SQL type issues.

    Delegates to the backend (Rust or Python) for base checks, then adds
    user-defined type validation on top. When ``SQL[UserType](...)`` is
    found and ``UserType`` is defined in the same source file, structural
    checks (column count, type, name) replace the stale-type string
    comparison.

    Args:
        source: Python source code
        dialect: SQL dialect
        schema_sql: Optional CREATE TABLE statements
        rules: Rule severity overrides

    Returns:
        List of diagnostic dicts with keys: rule, message, severity, line
    """
    import re

    from sqlty._type_extractor import extract_type_definitions

    # Run the backend check_source (Rust or Python)
    diagnostics = _backend_check_source(
        source, dialect=dialect, schema_sql=schema_sql, rules=rules
    )

    # Extract user-defined type definitions from source
    type_defs = extract_type_definitions(source)

    # Find all non-tuple SQL[T](...) annotations — these need special handling
    # because stale-type always fires for non-tuple types ("User" != "tuple[int, str]")
    non_tuple_lines: set[int] = set()
    for i, line in enumerate(source.splitlines(), 1):
        type_match = re.search(r'SQL\[(.+?)\]\(', line.strip())
        if type_match:
            annotated_type = type_match.group(1)
            if not annotated_type.startswith("tuple["):
                non_tuple_lines.add(i)

    if not non_tuple_lines:
        return diagnostics

    # Filter out stale-type for all non-tuple annotations (they're either
    # user-defined types with structural checks, or external types we can't validate)
    filtered = [
        d for d in diagnostics
        if not (d["rule"] == "stale-type" and d.get("line") in non_tuple_lines)
    ]

    # Add structural checks for user-defined types found in source
    if type_defs:
        filtered = _enhance_with_user_type_checks(
            filtered, source, type_defs, non_tuple_lines, dialect, schema_sql, rules
        )

    return filtered


def _enhance_with_user_type_checks(
    diagnostics: list[dict[str, str | int | None]],
    source: str,
    type_defs: dict,
    non_tuple_lines: set[int],
    dialect: str,
    schema_sql: str | None,
    rules: dict[str, str] | None,
) -> list[dict[str, str | int | None]]:
    """Add structural type checks for user-defined types.

    For each SQL[UserType](...) line where UserType is found in source,
    adds column-count-mismatch, column-type-mismatch, column-name-mismatch
    diagnostics based on the type's fields vs the SQL query columns.
    """
    import re

    from sqlty._checker import Checker
    from sqlty._lint import RuleConfig
    from sqlty._sql_analyzer import SQLAnalyzer

    # Build map of lines with user-type annotations found in source
    user_type_lines: dict[int, tuple[str, object]] = {}
    for i, line in enumerate(source.splitlines(), 1):
        if i not in non_tuple_lines:
            continue
        type_match = re.search(r'SQL\[(.+?)\]\(', line.strip())
        if type_match:
            annotated_type = type_match.group(1)
            if annotated_type in type_defs:
                user_type_lines[i] = (annotated_type, type_defs[annotated_type])

    if not user_type_lines:
        return diagnostics

    rule_config = RuleConfig(overrides=rules)

    # Create analyzer with schema if available
    import tempfile
    from pathlib import Path

    schema_path = None
    if schema_sql:
        tmp = Path(tempfile.mktemp(suffix=".sql"))
        tmp.write_text(schema_sql)
        schema_path = tmp

    try:
        analyzer = SQLAnalyzer(dialect=dialect, schema_path=schema_path)
        checker = Checker(analyzer=analyzer, rule_config=rule_config)
        has_schema = schema_sql is not None

        for i, line in enumerate(source.splitlines(), 1):
            if i not in user_type_lines:
                continue

            type_name, type_info = user_type_lines[i]
            stripped = line.strip()

            # Extract SQL content from wrapper
            sql_match = re.search(
                r'SQL\[.*?\]\(\s*["\'](.+?)["\']\s*\)', stripped, re.DOTALL
            )
            if not sql_match:
                continue

            sql_content = sql_match.group(1)
            if not sql_content.startswith("--sql"):
                continue

            # Strip --sql prefix
            sql_query = _strip_sql_prefix(sql_content)

            # Run structural checks
            query_diags = checker.check_query(
                sql=sql_query,
                annotation_fields=type_info.fields,
                type_name=type_name,
                has_schema=has_schema,
            )
            for d in query_diags:
                diagnostics.append({
                    "rule": d.rule,
                    "message": d.message,
                    "severity": d.severity.value,
                    "line": i,
                })
    finally:
        if schema_path and schema_path.exists():
            schema_path.unlink(missing_ok=True)

    return diagnostics


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


__all__ = [
    "check_source",
    "expand_star",
    "infer_type",
    "rewrite_source",
    "strip_source",
]

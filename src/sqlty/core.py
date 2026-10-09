"""Public core API — import-switching facade.

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

Backend selection via ``SQLTY_LIB`` environment variable:

- ``SQLTY_LIB=py``  — force the pure-Python backend (``_core_py``)
- ``SQLTY_LIB=rs``  — force the Rust backend (``_core``); fails if not compiled
- unset (default)   — auto: use Rust if available, otherwise Python

Both implementations must satisfy the same API contract
(see ``tests/test_core_conformance.py``).

``check_source`` is wrapped to add Python-side user-defined type checking
(TypedDict, NamedTuple, dataclass, Pydantic) on top of whichever backend
is active, since type extraction requires Python's ``ast`` module.
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from sqlty._lint import RuleConfig
    from sqlty._sql_analyzer import ColumnInfo
    from sqlty._type_extractor import TypeInfo

_SQLTY_LIB = os.environ.get("SQLTY_LIB")

if _SQLTY_LIB == "py":
    from sqlty._core_py import (
        check_source as _backend_check_source,
    )
    from sqlty._core_py import (
        expand_star,
        infer_type,
    )
    from sqlty._core_py import (
        rewrite_source as _backend_rewrite_source,
    )
    from sqlty._core_py import (
        strip_source as _backend_strip_source,
    )
elif _SQLTY_LIB == "rs":
    from sqlty._core import (
        check_source as _backend_check_source,
    )
    from sqlty._core import (
        expand_star,
        infer_type,
    )
    from sqlty._core import (
        rewrite_source as _backend_rewrite_source,
    )
    from sqlty._core import (
        strip_source as _backend_strip_source,
    )
else:
    try:
        from sqlty._core import (
            check_source as _backend_check_source,
        )
        from sqlty._core import (
            expand_star,
            infer_type,
        )
        from sqlty._core import (
            rewrite_source as _backend_rewrite_source,
        )
        from sqlty._core import (
            strip_source as _backend_strip_source,
        )
    except ImportError:
        from sqlty._core_py import (
            check_source as _backend_check_source,
        )
        from sqlty._core_py import (
            expand_star,
            infer_type,
        )
        from sqlty._core_py import (
            rewrite_source as _backend_rewrite_source,
        )
        from sqlty._core_py import (
            strip_source as _backend_strip_source,
        )


def rewrite_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    import_style: str | None = None,
) -> str:
    """Rewrite Python source: "--sql ..." -> SQL[T]("--sql ...").

    Delegates to the active backend (Rust or Python).

    Args:
        source: Python source code
        dialect: SQL dialect
        schema_sql: Optional CREATE TABLE statements
        import_style: Import style (e.g. ``"from sqlty import SQL as Q"``)

    Returns:
        Rewritten source code
    """
    return _backend_rewrite_source(
        source, dialect=dialect, schema_sql=schema_sql, import_style=import_style
    )


def strip_source(source: str, *, import_style: str | None = None) -> str:
    """Strip Python source: SQL[T]("--sql ...") -> "--sql ...".

    Delegates to the active backend (Rust or Python).

    Args:
        source: Python source code with SQL[T](...) wrappers
        import_style: Import style (e.g. ``"from sqlty import SQL as Q"``)

    Returns:
        Stripped source code
    """
    return _backend_strip_source(source, import_style=import_style)


def check_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
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
        import_style: Import style (e.g. ``"from sqlty import SQL as Q"``)

    Returns:
        List of diagnostic dicts with keys: rule, message, severity, line
    """
    import re

    from sqlty._rewriter import ImportStyle
    from sqlty._type_extractor import extract_type_definitions

    istyle = ImportStyle.parse(import_style) if import_style else ImportStyle.default()
    wrapper_name = istyle.wrapper_name
    escaped_wrapper = re.escape(wrapper_name)

    # Run the backend check_source (Rust or Python)
    diagnostics = _backend_check_source(
        source, dialect=dialect, schema_sql=schema_sql, rules=rules, import_style=import_style
    )

    # Extract user-defined type definitions from source
    type_defs = extract_type_definitions(source)

    # Find all non-tuple {wrapper_name}[T](...) annotations — these need special handling
    # because stale-type always fires for non-tuple types ("User" != "tuple[int, str]")
    non_tuple_lines: set[int] = set()
    for i, line in enumerate(source.splitlines(), 1):
        type_match = re.search(rf"{escaped_wrapper}\[(.+?)\]\(", line.strip())
        if type_match:
            annotated_type = type_match.group(1)
            if not annotated_type.startswith("tuple["):
                non_tuple_lines.add(i)

    if not non_tuple_lines:
        return diagnostics

    # Filter out stale-type for all non-tuple annotations (they're either
    # user-defined types with structural checks, or external types we can't validate)
    filtered = [
        d
        for d in diagnostics
        if not (d["rule"] == "stale-type" and d.get("line") in non_tuple_lines)
    ]

    # Add structural checks for user-defined types found in source
    if type_defs:
        filtered = _enhance_with_user_type_checks(
            filtered,
            source,
            type_defs,
            non_tuple_lines,
            dialect,
            schema_sql,
            rules,
            wrapper_name=wrapper_name,
        )

    return filtered


def _enhance_with_user_type_checks(
    diagnostics: list[dict[str, str | int | None]],
    source: str,
    type_defs: dict[str, TypeInfo],
    non_tuple_lines: set[int],
    dialect: str,
    schema_sql: str | None,
    rules: dict[str, str] | None,
    *,
    wrapper_name: str = "SQL",
) -> list[dict[str, str | int | None]]:
    """Add structural type checks for user-defined types.

    For each SQL[UserType](...) line where UserType is found in source,
    runs column-count-mismatch, column-type-mismatch, column-name-mismatch
    checks using runtime-resolved type hints when available.
    """
    import re

    from sqlty._lint import RuleConfig
    from sqlty._sql_analyzer import SQLAnalyzer

    escaped_wrapper = re.escape(wrapper_name)

    # Build map of lines with user-type annotations found in source
    user_type_lines: dict[int, tuple[str, TypeInfo]] = {}
    for i, line in enumerate(source.splitlines(), 1):
        if i not in non_tuple_lines:
            continue
        type_match = re.search(rf"{escaped_wrapper}\[(.+?)\]\(", line.strip())
        if type_match:
            annotated_type = type_match.group(1)
            if annotated_type in type_defs:
                user_type_lines[i] = (annotated_type, type_defs[annotated_type])

    if not user_type_lines:
        return diagnostics

    rule_config = RuleConfig(overrides=rules)

    # Create analyzer with schema if available
    import os
    import tempfile
    from pathlib import Path

    schema_path = None
    fd = None
    if schema_sql:
        fd, tmp_name = tempfile.mkstemp(suffix=".sql")
        tmp = Path(tmp_name)
        tmp.write_text(schema_sql)
        os.close(fd)
        schema_path = tmp

    try:
        analyzer = SQLAnalyzer(dialect=dialect, schema_path=schema_path)

        for i, line in enumerate(source.splitlines(), 1):
            if i not in user_type_lines:
                continue

            type_name, type_info = user_type_lines[i]
            stripped = line.strip()

            # Extract SQL content from wrapper
            sql_match = re.search(
                rf'{escaped_wrapper}\[.*?\]\(\s*["\'](.+?)["\']\s*\)', stripped, re.DOTALL
            )
            if not sql_match:
                continue

            sql_content = sql_match.group(1)
            if not sql_content.startswith("--sql"):
                continue

            # Strip --sql prefix and analyze the query
            sql_query = _strip_sql_prefix(sql_content)
            columns = analyzer.analyze_select(sql_query)
            if not columns:
                continue

            for rule, message in _structural_diagnostics(
                type_name, type_info, columns, rule_config
            ):
                if rule_config.is_enabled(rule):
                    diagnostics.append(
                        {
                            "rule": rule,
                            "message": message,
                            "severity": rule_config.severity(rule).value,
                            "line": i,
                        }
                    )
    finally:
        if schema_path and schema_path.exists():
            schema_path.unlink(missing_ok=True)

    return diagnostics


def _structural_diagnostics(
    type_name: str, info: TypeInfo, columns: list[ColumnInfo], rules: RuleConfig
) -> list[tuple[str, str]]:
    """Compare a user type with query columns, returning ``(rule, message)`` pairs.

    Fields match columns by position. Closed TypedDicts and those with
    ``extra_items`` match by key instead: ``NotRequired`` keys may be absent,
    and only ``extra_items`` admits undeclared columns (type-checked against it).
    A reported count mismatch suppresses the per-column checks.
    """
    from sqlty._type_extractor import check_type_compatibility

    fields = info.fields
    declared = dict(fields)
    lo = len(info.required) if info.keyed else len(fields)
    hi = len(fields) if not info.extra_items else len(columns)
    if not lo <= len(columns) <= hi and rules.is_enabled("column-count-mismatch"):
        expected = f"{len(fields)} fields"
        if info.extra_items:
            expected = f"{lo} required keys"
        elif lo < hi:
            expected = f"{lo} required and {hi} total keys"
        return [
            (
                "column-count-mismatch",
                f"{type_name} has {expected} but query returns {len(columns)} columns",
            )
        ]

    def type_mismatch(field: str, col: ColumnInfo) -> list[tuple[str, str]]:
        if not col.python_type or check_type_compatibility(info, field, col.python_type):
            return []
        declared_type = declared.get(field) or f"extra_items {info.extra_items}"
        return [
            (
                "column-type-mismatch",
                f"{type_name}.{field} is {declared_type} but query column "
                f"'{col.name}' infers {col.python_type}",
            )
        ]

    if not info.keyed:
        pairs = list(zip(fields, columns, strict=False))
        return [d for (field, _), col in pairs for d in type_mismatch(field, col)] + [
            ("column-name-mismatch", f"{type_name}.{field} doesn't match query column '{col.name}'")
            for (field, _), col in pairs
            if field != col.name
        ]

    known = [col for col in columns if col.name in declared or info.extra_items]
    unknown = [col for col in columns if col.name not in declared and not info.extra_items]
    names = {col.name for col in columns}
    missing = [key for key, _ in fields if key in info.required and key not in names]
    return (
        [d for col in known for d in type_mismatch(col.name, col)]
        + [
            ("column-name-mismatch", f"{type_name} has no key for query column '{col.name}'")
            for col in unknown
        ]
        + [
            ("column-name-mismatch", f"{type_name}.{key} is required but missing from query")
            for key in missing
        ]
    )


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

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
    from sqlty._core import (  # type: ignore[assignment]
        check_source as _backend_check_source,
    )
    from sqlty._core import (
        expand_star,
        infer_type,
    )
    from sqlty._core import (  # type: ignore[assignment]
        rewrite_source as _backend_rewrite_source,
    )
    from sqlty._core import (  # type: ignore[assignment]
        strip_source as _backend_strip_source,
    )
else:
    try:
        from sqlty._core import (  # type: ignore[assignment]
            check_source as _backend_check_source,
        )
        from sqlty._core import (
            expand_star,
            infer_type,
        )
        from sqlty._core import (  # type: ignore[assignment]
            rewrite_source as _backend_rewrite_source,
        )
        from sqlty._core import (  # type: ignore[assignment]
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
    from sqlty._type_extractor import check_type_compatibility

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
    import tempfile
    from pathlib import Path

    schema_path = None
    if schema_sql:
        tmp = Path(tempfile.mktemp(suffix=".sql"))
        tmp.write_text(schema_sql)
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

            fields = type_info.fields  # [(name, type_str), ...]

            # Check column count
            if rule_config.is_enabled("column-count-mismatch"):
                if len(fields) != len(columns):
                    diagnostics.append(
                        {
                            "rule": "column-count-mismatch",
                            "message": (
                                f"{type_name} has {len(fields)} fields "
                                f"but query returns {len(columns)} columns"
                            ),
                            "severity": rule_config.severity("column-count-mismatch").value,
                            "line": i,
                        }
                    )
                    continue  # Skip type/name checks when counts differ

            # Check column types — using runtime type checking
            if rule_config.is_enabled("column-type-mismatch"):
                for (field_name, _field_type_str), col in zip(fields, columns, strict=False):
                    if col.python_type and not check_type_compatibility(
                        type_info, field_name, col.python_type
                    ):
                        diagnostics.append(
                            {
                                "rule": "column-type-mismatch",
                                "message": (
                                    f"{type_name}.{field_name} is "
                                    f"{_field_type_str} but query column "
                                    f"'{col.name}' infers {col.python_type}"
                                ),
                                "severity": rule_config.severity("column-type-mismatch").value,
                                "line": i,
                            }
                        )

            # Check column names
            if rule_config.is_enabled("column-name-mismatch"):
                for (field_name, _), col in zip(fields, columns, strict=False):
                    if field_name != col.name:
                        diagnostics.append(
                            {
                                "rule": "column-name-mismatch",
                                "message": (
                                    f"{type_name}.{field_name} doesn't match "
                                    f"query column '{col.name}'"
                                ),
                                "severity": rule_config.severity("column-name-mismatch").value,
                                "line": i,
                            }
                        )
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

"""The SQL engine behind the CLI, implemented in Rust (``sqlty._core``).

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

``check_source`` adds the checks of user-defined row types (TypedDict,
NamedTuple, dataclass, Pydantic) on top of the engine's, since resolving them
needs the Python runtime.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, NamedTuple

from sqlty._core import analyze_select as _analyze_select
from sqlty._core import check_source as _check_source
from sqlty._core import (
    expand_star,
    fix_source,
    infer_type,
    returns_rows,
    rewrite_source,
    strip_source,
)
from sqlty._core import schema_skipped_tables as _schema_skipped_tables
from sqlty._core import wrapped_queries as _wrapped_queries

if TYPE_CHECKING:
    from sqlty._lint import RuleConfig
    from sqlty._type_extractor import TypeInfo


class ColumnInfo(NamedTuple):
    """A result column; types are ``None`` when unknown."""

    name: str
    python_type: str | None
    sql_type: str | None


def analyze_select(
    sql: str, *, dialect: str = "postgres", schema_sql: str | None = None
) -> list[ColumnInfo] | None:
    """Result columns of a query, or ``None`` when it returns no rows or doesn't parse.

    Example:
        >>> analyze_select("SELECT id::INTEGER, COUNT(*) AS n FROM users")
        [ColumnInfo(name='id', python_type='int', sql_type='INT'), ColumnInfo(name='n', python_type='int', sql_type='INTEGER')]

    Raises:
        RuntimeError: ``SUM(column)`` names a column missing from a schema table.
    """
    columns = _analyze_select(sql, dialect=dialect, schema_sql=schema_sql)
    return None if columns is None else [ColumnInfo(*column) for column in columns]


def schema_skipped_tables(schema_sql: str, *, dialect: str = "postgres") -> list[str]:
    """Names of schema tables that can't be parsed, so their columns are unknown.

    A table is retried without its constraints and ``CHECK`` / ``REFERENCES``
    clauses first; only tables that still fail are listed.

    Example:
        >>> schema_skipped_tables("CREATE TABLE ok (id INT); CREATE TABLE bad (id INT DEFAULT %s)")
        ['bad']
    """
    return _schema_skipped_tables(schema_sql, dialect=dialect)


def check_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    untyped: bool = False,
) -> list[dict[str, str | int | None]]:
    """Check Python source for SQL type issues.

    When ``SQL[UserType](...)`` names a type defined in the same source,
    structural checks (column count, type, name, TypedDict keys) run instead
    of ``stale-type``.

    Returns:
        List of diagnostic dicts with keys: rule, message, severity, line

    Raises:
        ValueError: unsupported ``import_style``
    """
    from sqlty._lint import RuleConfig
    from sqlty._type_extractor import extract_type_definitions

    diagnostics = _check_source(
        source,
        dialect=dialect,
        schema_sql=schema_sql,
        rules=rules,
        import_style=import_style,
        untyped=untyped,
    )
    user_queries = [
        (line, name, query)
        for line, name, query in _wrapped_queries(source, import_style=import_style)
        if not name.startswith("tuple[")
    ]
    if not user_queries:
        return diagnostics
    type_defs = extract_type_definitions(source)
    user_queries = [q for q in user_queries if q[1] in type_defs]
    if not user_queries:
        return diagnostics

    rule_config = RuleConfig(overrides=rules)
    if rule_config.is_enabled("typeddict-inheritance"):
        severity = rule_config.severity("typeddict-inheritance").value
        for info in _typeddict_lineage([name for _, name, _ in user_queries], type_defs):
            diagnostics += [
                {
                    "rule": "typeddict-inheritance",
                    "message": message,
                    "severity": severity,
                    "line": info.lineno,
                }
                for message in info.errors
            ]

    for line, name, query in user_queries:
        if query is None:
            continue
        try:
            columns = analyze_select(query, dialect=dialect, schema_sql=schema_sql)
        except RuntimeError:
            continue
        if not columns:
            continue
        for rule, message in _structural_diagnostics(name, type_defs[name], columns, rule_config):
            if rule_config.is_enabled(rule):
                diagnostics.append(
                    {
                        "rule": rule,
                        "message": message,
                        "severity": rule_config.severity(rule).value,
                        "line": line,
                    }
                )
    return diagnostics


def _typeddict_lineage(names: list[str], type_defs: dict[str, TypeInfo]) -> list[TypeInfo]:
    """``names`` and their same-source TypedDict bases, bases first, each once."""
    seen: dict[str, TypeInfo] = {}

    def visit(name: str) -> None:
        if name in seen or (info := type_defs.get(name)) is None:
            return
        for base in info.bases:
            visit(base)
        seen[name] = info

    for name in names:
        visit(name)
    return list(seen.values())


def _structural_diagnostics(
    type_name: str, info: TypeInfo, columns: list[ColumnInfo], rules: RuleConfig
) -> list[tuple[str, str]]:
    """Compare a user type with query columns, returning ``(rule, message)`` pairs.

    TypedDicts, dataclasses and Pydantic models match columns to fields by
    name, as ``dict_row`` and ``class_row`` pass them: fields with a default
    (``NotRequired`` keys) may be absent, and a column without a field is a
    violation, except that Pydantic models ignore it and only ``extra_items``
    admits it (type-checked against it). Key-set violations are
    ``typeddict-key-mismatch``. NamedTuples match by position.
    A reported count mismatch suppresses the per-column checks.
    """
    from sqlty._type_extractor import check_type_compatibility

    fields = info.fields
    declared = dict(fields)
    by_name = info.kind != "NamedTuple"
    ignores_extra = info.kind == "BaseModel"
    unit = "keys" if info.kind == "TypedDict" else "fields"
    lo = len(info.required) if by_name else len(fields)
    hi = len(columns) if info.extra_items or ignores_extra else len(fields)
    if not lo <= len(columns) <= hi and rules.is_enabled("column-count-mismatch"):
        expected = f"{len(fields)} fields"
        if info.extra_items or ignores_extra:
            expected = f"{lo} required {unit}"
        elif lo < hi:
            expected = f"{lo} required and {hi} total {unit}"
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

    if not by_name:
        pairs = list(zip(fields, columns, strict=False))
        return [d for (field, _), col in pairs for d in type_mismatch(field, col)] + [
            ("column-name-mismatch", f"{type_name}.{field} doesn't match query column '{col.name}'")
            for (field, _), col in pairs
            if field != col.name
        ]

    admits_extra = info.extra_items or ignores_extra
    known = [col for col in columns if col.name in declared or info.extra_items]
    unknown = [col for col in columns if col.name not in declared and not admits_extra]
    names = {col.name for col in columns}
    missing = [key for key, _ in fields if key in info.required and key not in names]
    noun = "key" if info.kind == "TypedDict" else "field"
    return (
        [d for col in known for d in type_mismatch(col.name, col)]
        + [
            ("typeddict-key-mismatch", f"{type_name} has no {noun} for query column '{col.name}'")
            for col in unknown
        ]
        + [
            ("typeddict-key-mismatch", f"{type_name}.{key} is required but missing from query")
            for key in missing
        ]
    )


__all__ = [
    "ColumnInfo",
    "analyze_select",
    "check_source",
    "expand_star",
    "fix_source",
    "infer_type",
    "returns_rows",
    "rewrite_source",
    "schema_skipped_tables",
    "strip_source",
]

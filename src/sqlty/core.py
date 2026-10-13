"""The SQL engine behind the CLI, implemented in Rust (``sqlty._core``).

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

``check_source`` adds the checks of user-defined row types (TypedDict,
NamedTuple, dataclass, Pydantic) on top of the engine's, since resolving them
needs the Python runtime.
"""

from __future__ import annotations

import ast
import typing
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
from sqlty._helpers import HelperIndex, find_helper_calls
from sqlty._source import ParsedSource

if TYPE_CHECKING:
    from sqlty._helpers import HelperCall
    from sqlty._lint import RuleConfig
    from sqlty._type_extractor import TypeInfo


_CONTAINERS = (dict, list, set, frozenset, tuple)


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
    source: str | ParsedSource,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    helpers: HelperIndex | None = None,
    module: str | None = None,
) -> list[dict[str, str | int | None]]:
    """Check Python source for SQL type issues.

    When ``SQL[UserType](...)`` names a type defined in the same source,
    structural checks (column count, type, name, TypedDict keys) run instead
    of ``stale-type``.

    A string literal passed to a parameter annotated
    ``Annotated[LiteralString, SQL]`` is a query too, checked against the row
    type passed to the ``type[T]`` parameter of the same helper. Helpers
    defined in ``source`` count; for those of other modules pass ``helpers``
    (a ``HelperIndex``) and the ``module`` name of ``source``, which resolves
    relative imports.

    A ``# sqlty: ignore`` comment on a diagnostic's line drops it; ``# sqlty:
    ignore[rule-a, rule-b]`` drops only those rules. ``unused-ignore`` reports
    comments that dropped nothing. Comments are found with ``tokenize``, so a
    ``#`` inside a string is not one.

    ``source`` may be a ``ParsedSource``: the file is then parsed and walked at
    most once for all of these checks, and not at all when it holds no query.
    Build one per file when checking many (see ``sqlty._source``).

    Example:
        >>> check_source('q = SQL[tuple[str]]("--sql\\nSELECT * FROM users")  # sqlty: ignore[select-star-no-schema]')
        []

    Returns:
        List of diagnostic dicts with keys: rule, message, severity, line

    Raises:
        ValueError: unsupported ``import_style``
    """
    parsed = ParsedSource.of(source, module)
    diagnostics = _check_diagnostics(
        parsed,
        dialect=dialect,
        schema_sql=schema_sql,
        rules=rules,
        import_style=import_style,
        untyped=untyped,
        helpers=helpers,
        module=module,
    )
    return _apply_suppressions(parsed, diagnostics, rules)


def _check_diagnostics(
    parsed: ParsedSource,
    *,
    dialect: str,
    schema_sql: str | None,
    rules: dict[str, str] | None,
    import_style: str | None,
    untyped: bool,
    helpers: HelperIndex | None,
    module: str | None,
) -> list[dict[str, str | int | None]]:
    """Every diagnostic before inline suppression."""
    from sqlty._lint import RuleConfig
    from sqlty._type_extractor import extract_type_definitions

    diagnostics = _check_source(
        parsed.text,
        dialect=dialect,
        schema_sql=schema_sql,
        rules=rules,
        import_style=import_style,
        untyped=untyped,
    )
    calls = find_helper_calls(parsed, helpers, module)
    queries = [
        (line, name, query)
        for line, name, query in (
            _wrapped_queries(parsed.text, import_style=import_style)
            if parsed.may_wrap(import_style)
            else []
        )
        if not name.startswith("tuple[")
    ]
    if not (queries or calls):
        return diagnostics
    rule_config = RuleConfig(overrides=rules)

    def report(rule: str, message: str, line: int) -> None:
        if rule_config.is_enabled(rule):
            diagnostics.append(
                {
                    "rule": rule,
                    "message": message,
                    "severity": rule_config.severity(rule).value,
                    "line": line,
                }
            )

    named = [(call.line, ast.unparse(call.row), call.query) for call in calls if call.row]
    type_defs = extract_type_definitions(parsed) if queries or named else {}
    user_queries = [q for q in [*queries, *named] if q[1] in type_defs]

    if rule_config.is_enabled("typeddict-inheritance"):
        for info in _typeddict_lineage([name for _, name, _ in user_queries], type_defs):
            for message in info.errors:
                report("typeddict-inheritance", message, info.lineno)

    for call in calls:
        upper = call.query.upper()
        if "SELECT *" in upper or "SELECT\n*" in upper:
            if schema_sql is not None:
                report(
                    "select-star",
                    "SELECT * used \u2014 expand to explicit column list (joins need a manual edit)",
                    call.line,
                )
            else:
                report("select-star-no-schema", "SELECT * used but no schema available", call.line)

    for line, name, query in user_queries:
        if (columns := _columns(query, dialect, schema_sql)) is None:
            continue
        for rule, message in _structural_diagnostics(name, type_defs[name], columns, rule_config):
            report(rule, message, line)

    for call in calls:
        if call.row is None or ast.unparse(call.row) in type_defs:
            continue
        if (columns := _columns(call.query, dialect, schema_sql)) is None:
            continue
        for rule, message in _value_diagnostics(call, columns):
            report(rule, message, call.line)
    return diagnostics


def _columns(query: str | None, dialect: str, schema_sql: str | None) -> list[ColumnInfo] | None:
    """Columns of a query, ``None`` when it has none or cannot be analysed."""
    if query is None:
        return None
    try:
        return analyze_select(query, dialect=dialect, schema_sql=schema_sql) or None
    except RuntimeError:
        return None


def _value_diagnostics(call: HelperCall, columns: list[ColumnInfo]) -> list[tuple[str, str]]:
    """Compare the row type of a helper call with the query columns.

    A scalar type takes one column, ``tuple[A, B]`` one column per item. Other
    row types (an unknown class, ``dict``, ``tuple[int, ...]``) are not checked.
    """
    from sqlty._type_extractor import _accepts, _ast_type

    row, aliases = call.row, call.typing_aliases
    if row is None:
        return []
    text = ast.unparse(row)
    if isinstance(row, ast.Subscript) and _ast_type(row.value, aliases) is tuple:
        items = list(row.slice.elts) if isinstance(row.slice, ast.Tuple) else [row.slice]
        if not items or any(isinstance(i, ast.Constant) and i.value is ... for i in items):
            return []
        if len(items) != len(columns):
            return [
                (
                    "column-count-mismatch",
                    f"{text} has {len(items)} items but query returns {len(columns)} columns",
                )
            ]
        targets = [
            (f"{text} item {i} is {ast.unparse(item)}", ast.unparse(item), _ast_type(item, aliases))
            for i, item in enumerate(items)
        ]
    else:
        resolved = _ast_type(row, aliases)
        if resolved is None or resolved in _CONTAINERS or typing.get_args(resolved):
            return []
        if len(columns) != 1:
            return [
                (
                    "column-count-mismatch",
                    f"{text} takes 1 column but query returns {len(columns)} columns",
                )
            ]
        targets = [(f"{text} is expected", text, resolved)]
    return [
        (
            "column-type-mismatch",
            f"{label} but query column '{col.name}' infers {col.python_type}",
        )
        for (label, annotation, resolved), col in zip(targets, columns, strict=True)
        if resolved is not None
        and col.python_type
        and not _accepts(resolved, annotation, col.python_type)
    ]


def _apply_suppressions(
    parsed: ParsedSource,
    diagnostics: list[dict[str, str | int | None]],
    rules: dict[str, str] | None,
) -> list[dict[str, str | int | None]]:
    """Drop diagnostics suppressed by a comment on their line; add ``unused-ignore``."""
    from sqlty._lint import RuleConfig

    ignores = parsed.ignores
    if not ignores:
        return diagnostics

    kept: list[dict[str, str | int | None]] = []
    used: set[tuple[int, str | None]] = set()
    for diag in diagnostics:
        line = diag["line"]
        ignore = ignores.get(line) if isinstance(line, int) else None
        if ignore is None or (ignore.rules is not None and diag["rule"] not in ignore.rules):
            kept.append(diag)
        else:
            used.add((ignore.line, None if ignore.rules is None else str(diag["rule"])))

    rule_config = RuleConfig(overrides=rules)
    if rule_config.is_enabled("unused-ignore"):
        severity = rule_config.severity("unused-ignore").value
        for ignore in ignores.values():
            unused: list[str | None]
            if ignore.rules is None:
                unused = [None] if (ignore.line, None) not in used else []
            else:
                unused = [name for name in ignore.rules if (ignore.line, name) not in used]
            for name in unused:
                written = "# sqlty: ignore" if name is None else f"# sqlty: ignore[{name}]"
                kept.append(
                    {
                        "rule": "unused-ignore",
                        "message": f"'{written}' suppresses no diagnostic",
                        "severity": severity,
                        "line": ignore.line,
                    }
                )
    return kept


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
    "HelperIndex",
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

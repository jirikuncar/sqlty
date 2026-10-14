"""The SQL engine behind the CLI, implemented in Rust (``sqlty._core``).

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

``check_source`` adds the checks of user-defined row types (TypedDict,
NamedTuple, dataclass, Pydantic) on top of the engine's, since resolving them
needs the Python runtime.
"""

from __future__ import annotations

import ast
import re
import typing
from collections import Counter
from collections.abc import Iterable
from typing import TYPE_CHECKING, NamedTuple

from sqlty._core import analyze_select as _analyze_select
from sqlty._core import check_source as _check_source
from sqlty._core import (
    expand_star,
    infer_type,
    returns_rows,
    rewrite_source,
    strip_source,
)
from sqlty._core import fix_source as _fix_source
from sqlty._core import parse_error as _parse_error
from sqlty._core import query_sites as _query_sites
from sqlty._core import schema_skipped_objects as _schema_skipped_objects
from sqlty._core import schema_skipped_tables as _schema_skipped_tables
from sqlty._core import wrapped_queries as _wrapped_queries
from sqlty._helpers import HelperIndex, find_helper_calls
from sqlty._source import ParsedSource

if TYPE_CHECKING:
    from sqlty._helpers import HelperCall
    from sqlty._lint import RuleConfig
    from sqlty._type_extractor import TypeInfo


_CONTAINERS = (dict, list, set, frozenset, tuple)
# The `--sql` marker that opens a query string, as the engine reads it.
_MARKER = re.compile(r"\s*--\s?sql(?:\s|$)")


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
        [ColumnInfo(name='id', python_type='int', sql_type='INT'), ColumnInfo(name='n', python_type='int', sql_type='BIGINT')]

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


def schema_skipped_objects(schema_sql: str, *, dialect: str = "postgres") -> list[tuple[str, str]]:
    """``(kind, name)`` of schema tables and views that can't be parsed, so their columns are unknown.

    ``kind`` is ``"table"`` or ``"view"``. Unparseable tables are retried without
    their constraints first, as in :func:`schema_skipped_tables`.

    Example:
        >>> schema_skipped_objects("CREATE TABLE ok (id INT); CREATE VIEW v AS SELECT id FROM (")
        [('view', 'v')]
    """
    return _schema_skipped_objects(schema_sql, dialect=dialect)


def fix_source(
    source: str | ParsedSource,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    rules: dict[str, str] | None = None,
) -> str:
    """Apply what ``sqlty check --fix`` fixes; rules set to ``"ignore"`` are skipped.

    A fix for a rule is not applied to a query whose reported line carries
    ``# sqlty: ignore`` or ``# sqlty: ignore[rule]`` naming that rule, so a
    suppressed ``select-star`` is not expanded. Other rules on the line are fixed.

    Example:
        >>> fix_source('q = "--sql SELECT 1"  # sqlty: ignore[inline-sql-comment]')
        'q = "--sql SELECT 1"  # sqlty: ignore[inline-sql-comment]'

    Raises:
        ValueError: unsupported ``import_style``
    """
    parsed = ParsedSource.of(source)
    return _fix_source(
        parsed.text,
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=import_style,
        untyped=untyped,
        rules=rules,
        suppressed={
            line: None if ignore.rules is None else list(ignore.rules)
            for line, ignore in parsed.ignores.items()
        }
        or None,
    )


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
    type_modules: Iterable[str] | None = None,
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
    relative imports. The same index resolves row type fields annotated with a
    type imported from another module of the index (``NewType``, ``Literal``
    and ``type`` aliases, ``X = int``), read from its source and never imported.
    Row type resolution executes only imports of the standard library, of
    ``typing``, ``pydantic`` and the like, and of ``type_modules`` (module
    names, submodules included); any other name is unknown, so no import of the
    checked application runs.

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
        module=module or parsed.module,
        type_modules=type_modules,
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
    type_modules: Iterable[str] | None = None,
) -> list[dict[str, str | int | None]]:
    """Every diagnostic before inline suppression."""
    from sqlty._lint import RuleConfig
    from sqlty._type_extractor import extract_type_definitions

    diagnostics = (
        _check_source(
            parsed.text,
            dialect=dialect,
            schema_sql=schema_sql,
            rules=rules,
            import_style=import_style,
            untyped=untyped,
        )
        if parsed.may_need_rust_check(import_style)
        else []
    )
    found = find_helper_calls(parsed, helpers, module)
    calls = [call for call in found if call.query is not None]
    queries = [
        (line, name, query)
        for line, name, query in (
            _wrapped_queries(parsed.text, import_style=import_style)
            if parsed.may_wrap(import_style)
            else []
        )
        if not name.startswith("tuple[")
    ]
    if not (queries or found):
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

    for call in found:
        if call.query is None and not _is_any(call):
            report(
                "unchecked-query",
                "psycopg query is not built from literals only, so it is not checked",
                call.line,
            )

    unparsed: dict[str, str] = {}

    def unparse(row: ast.expr) -> str:
        """``ast.unparse(row)``, once per distinct source span in the file."""
        if (key := parsed.segment(row)) is None:
            return ast.unparse(row)
        if key not in unparsed:
            unparsed[key] = ast.unparse(row)
        return unparsed[key]

    rows = [unparse(call.row) if call.row else None for call in calls]
    named = [(call.line, row, call.query) for call, row in zip(calls, rows, strict=True) if row]
    type_defs = (
        extract_type_definitions(parsed, helpers, module, type_modules) if queries or named else {}
    )
    user_queries = [q for q in [*queries, *named] if q[1] in type_defs]

    if rule_config.is_enabled("typeddict-inheritance"):
        for info in _typeddict_lineage([name for _, name, _ in user_queries], type_defs):
            for message in info.errors:
                report("typeddict-inheritance", message, info.lineno)

    for call in calls:
        upper = (call.query or "").upper()
        if "SELECT *" in upper or "SELECT\n*" in upper:
            if schema_sql is not None:
                report(
                    "select-star",
                    "SELECT * used \u2014 expand to explicit column list (joins need a manual edit)",
                    call.line,
                )
            else:
                report("select-star-no-schema", "SELECT * used but no schema available", call.line)

    for call in calls if rule_config.is_enabled("unparsed-query") else []:
        if (
            call.query is not None
            and not _MARKER.match(call.query)
            and (error := _parse_error(call.query, dialect=dialect))
        ):
            report("unparsed-query", f"query does not parse: {error}", call.line)

    for line, name, query in user_queries:
        if (columns := _columns(query, dialect, schema_sql)) is None:
            continue
        for rule, message in _structural_diagnostics(name, type_defs[name], columns, rule_config):
            report(rule, message, line)

    shapes: dict[str, _RowShape | None] = {}
    for call, row in zip(calls, rows, strict=True):
        if call.row is None or row is None or row in type_defs:
            continue
        if (columns := _columns(call.query, dialect, schema_sql)) is None:
            continue
        if row not in shapes:
            shapes[row] = _row_shape(call.row, row, call.typing_aliases)
        for rule, message in _value_diagnostics(shapes[row], columns):
            report(rule, message, call.line)
    return diagnostics


def query_statistics(
    source: str | ParsedSource,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    import_style: str | None = None,
    helpers: HelperIndex | None = None,
    module: str | None = None,
) -> Counter[str]:
    """How the queries of ``source`` fare, as ``sqlty check --statistics`` counts them.

    Counts ``typed`` queries, ``partially typed`` ones (a column's type is
    unknown), ``unparsed`` ones, and skipped strings by rule
    (``fstring-skipped``, ``tstring-skipped``, ``unchecked-query``). Queries
    that return no rows are not counted.

    Example:
        >>> query_statistics('q = SQL[tuple[int]]("--sql\\\\nSELECT id::INTEGER FROM users")')
        Counter({'typed': 1})
    """
    parsed = ParsedSource.of(source, module)
    counts: Counter[str] = Counter()
    for _, kind, text in _query_sites(parsed.text, import_style=import_style):
        if kind != "query":
            counts[kind] += 1
        elif outcome := _outcome(text, dialect, schema_sql):
            counts[outcome] += 1
    for call in find_helper_calls(parsed, helpers, module):
        if call.query is None:
            if not _is_any(call):
                counts["unchecked-query"] += 1
        elif not _MARKER.match(call.query) and (
            outcome := _outcome(call.query, dialect, schema_sql)
        ):
            counts[outcome] += 1
    return counts


def _is_any(call: HelperCall) -> bool:
    """Whether the row type is ``Any``: an explicit opt-out of checks."""
    from sqlty._type_extractor import _ast_type

    return call.row is not None and _ast_type(call.row, call.typing_aliases) is typing.Any


def _outcome(query: str, dialect: str, schema_sql: str | None) -> str | None:
    """``typed``, ``partially typed`` or ``unparsed``; ``None`` when ``query`` returns no rows."""
    if _parse_error(query, dialect=dialect) is not None:
        return "unparsed"
    if not returns_rows(query, dialect=dialect):
        return None
    try:
        columns = analyze_select(query, dialect=dialect, schema_sql=schema_sql)
    except RuntimeError:
        columns = None
    if columns is not None and all(column.python_type is not None for column in columns):
        return "typed"
    return "partially typed"


def _columns(query: str | None, dialect: str, schema_sql: str | None) -> list[ColumnInfo] | None:
    """Columns of a query, ``None`` when it has none or cannot be analysed."""
    if query is None:
        return None
    try:
        return analyze_select(query, dialect=dialect, schema_sql=schema_sql) or None
    except RuntimeError:
        return None


class _RowShape(NamedTuple):
    """The row type of helper calls, resolved once per file for every call that spells it."""

    text: str
    is_tuple: bool
    targets: list[tuple[str, str, typing.Any]]
    accepts: dict[tuple[int, str], bool]


def _row_shape(row: ast.expr, text: str, aliases: dict[str, str]) -> _RowShape | None:
    """What each column of a helper call's row type ``text`` must be; ``None`` when unchecked.

    A scalar type takes one column, ``tuple[A, B]`` one column per item. Other
    row types (an unknown class, ``dict``, ``tuple[int, ...]``) are not checked.
    """
    from sqlty._type_extractor import _ast_type

    if isinstance(row, ast.Subscript) and _ast_type(row.value, aliases) is tuple:
        items = list(row.slice.elts) if isinstance(row.slice, ast.Tuple) else [row.slice]
        if not items or any(isinstance(i, ast.Constant) and i.value is ... for i in items):
            return None
        annotations = [ast.unparse(item) for item in items]
        targets = [
            (f"{text} item {i} is {annotation}", annotation, _ast_type(item, aliases))
            for i, (item, annotation) in enumerate(zip(items, annotations, strict=True))
        ]
        return _RowShape(text, True, targets, {})
    resolved = _ast_type(row, aliases)
    if resolved is None or resolved in _CONTAINERS or typing.get_args(resolved):
        return None
    return _RowShape(text, False, [(f"{text} is expected", text, resolved)], {})


def _value_diagnostics(shape: _RowShape | None, columns: list[ColumnInfo]) -> list[tuple[str, str]]:
    """Compare the row type of a helper call with the query columns."""
    from sqlty._type_extractor import _accepts

    if shape is None:
        return []
    if len(shape.targets) != len(columns):
        takes = f"has {len(shape.targets)} items" if shape.is_tuple else "takes 1 column"
        return [
            (
                "column-count-mismatch",
                f"{shape.text} {takes} but query returns {len(columns)} columns",
            )
        ]
    found = []
    for i, ((label, annotation, resolved), col) in enumerate(
        zip(shape.targets, columns, strict=True)
    ):
        if resolved is None or not col.python_type:
            continue
        key = (i, col.python_type)
        if key not in shape.accepts:
            shape.accepts[key] = _accepts(resolved, annotation, col.python_type)
        if not shape.accepts[key]:
            found.append(
                (
                    "column-type-mismatch",
                    f"{label} but query column '{col.name}' infers {col.python_type}",
                )
            )
    return found


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
    lo = len(info.required - info.opaque) if by_name else len(fields)
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

    admits_extra = info.extra_items or (ignores_extra and not info.forbid) or info.opaque
    owners = [(col, info.field_for(col.name)) for col in columns]
    known = [(col, field or col.name) for col, field in owners if field or info.extra_items]
    unknown = [col for col, field in owners if field is None and not admits_extra]
    names = {col.name for col in columns}
    missing = [
        info.column_names(key)[0]
        for key, _ in fields
        if key in info.required
        and key not in info.opaque
        and not names.intersection(info.column_names(key))
    ]
    noun = "key" if info.kind == "TypedDict" else "field"
    return (
        [d for col, field in known for d in type_mismatch(field, col)]
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
    "schema_skipped_objects",
    "schema_skipped_tables",
    "strip_source",
]

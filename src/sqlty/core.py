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
from collections.abc import Iterable, Mapping
from typing import TYPE_CHECKING, NamedTuple, cast

from sqlty._core import analyze_columns as _analyze_columns
from sqlty._core import analyze_params as _analyze_params
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
from sqlty._core import query_diagnostics as _query_diagnostics
from sqlty._core import query_sites as _query_sites
from sqlty._core import schema_skipped_objects as _schema_skipped_objects
from sqlty._core import schema_skipped_tables as _schema_skipped_tables
from sqlty._core import select_star_finding as _select_star_finding
from sqlty._core import wrapped_queries as _wrapped_queries
from sqlty._drivers import find_driver_calls
from sqlty._helpers import HelperIndex, find_helper_calls
from sqlty._lint import NAME_RULES, Diagnostic
from sqlty._params import mismatch
from sqlty._source import ParsedSource, is_any

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


class _RowColumn(NamedTuple):
    """A result column with whether it can be NULL (``None`` when unknown), for row type checks."""

    name: str
    python_type: str | None
    sql_type: str | None
    nullable: bool | None
    json_shape: str | None = None
    """The shape of the JSON the column builds, as the engine spells it (see ``_json_shape``)."""


def analyze_select(
    sql: str, *, dialect: str = "postgres", schema_sql: str | None = None
) -> list[ColumnInfo] | None:
    """Result columns of a query, or ``None`` when it returns no rows or doesn't parse.

    A column that can be NULL has a ``T | None`` Python type.

    Example:
        >>> analyze_select("SELECT id::INTEGER, COUNT(*) AS n FROM users")
        [ColumnInfo(name='id', python_type='int', sql_type='INT'), ColumnInfo(name='n', python_type='int', sql_type='BIGINT')]
        >>> schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT)"
        >>> [c.python_type for c in analyze_select("SELECT id, name FROM users", schema_sql=schema)]
        ['int', 'str | None']

    Raises:
        RuntimeError: ``SUM(column)`` names a column missing from a schema table.
    """
    columns = _analyze_select(sql, dialect=dialect, schema_sql=schema_sql)
    return None if columns is None else [ColumnInfo(*column) for column in columns]


def json_shapes(
    sql: str, *, dialect: str = "postgres", schema_sql: str | None = None
) -> list[tuple[str, str]]:
    """The shape of the JSON each column builds, as ``(column, shape)`` pairs.

    Only PostgreSQL columns built with ``jsonb_build_object()``, ``jsonb_agg()``,
    ``to_jsonb()``, ``->`` and the like have one. The column's Python type stays ``Any``;
    ``json-shape-mismatch`` checks the shape against the row type.

    Example:
        >>> json_shapes("SELECT jsonb_build_object('id', 1, 'name', 'x') AS doc")
        [('doc', '{id: integer, name: string}')]
        >>> json_shapes("SELECT coalesce(jsonb_agg(1), '[]') AS xs")
        [('xs', '[integer]')]
    """
    from sqlty._json_shape import render

    try:
        columns = _analyze_columns(sql, dialect=dialect, schema_sql=schema_sql)
    except RuntimeError:
        return []
    return [(c[0], render(c[4])) for c in columns or [] if c[4] is not None]


class ParamInfo(NamedTuple):
    """A query parameter. ``name`` is set for ``%(name)s`` and ``:name``; types are ``None`` when unknown.

    ``nullable`` is ``False`` when ``None`` cannot be passed (the parameter is stored in a
    ``NOT NULL`` column), ``True`` when it can, ``None`` when the query does not say.
    ``cast`` is ``True`` when the type is that of an explicit cast (``%s::int``), which
    accepts any value.
    """

    number: int
    name: str | None
    python_type: str | None
    sql_type: str | None
    nullable: bool | None
    cast: bool = False


def analyze_params(
    sql: str, *, dialect: str = "postgres", schema_sql: str | None = None
) -> list[ParamInfo] | None:
    """Parameters of a query with the type each takes from its context, or ``None`` when it doesn't parse.

    Placeholders are ``%s``, ``%(name)s``, ``$1``, ``?`` and ``:name`` (``?`` outside
    PostgreSQL, ``:name`` in SQLite). A parameter takes its type from the column it is
    compared with or stored in, a cast, ``LIMIT``, ``IN`` lists, function arguments or an
    array (``= ANY(%s)`` is ``list[T]``). ``number`` is 1-based, in order of appearance; a
    named parameter used twice is one.

    Example:
        >>> schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, age INTEGER)"
        >>> analyze_params("SELECT name FROM users WHERE id = %s AND age > %(min)s", schema_sql=schema)
        [ParamInfo(number=1, name=None, python_type='int', sql_type='INT', nullable=None, cast=False), ParamInfo(number=2, name='min', python_type='int', sql_type='INT', nullable=None, cast=False)]
        >>> [(p.python_type, p.nullable) for p in analyze_params("INSERT INTO users (id, name, age) VALUES ($1, $2, $3)", schema_sql=schema)]
        [('int', False), ('str', False), ('int', True)]
        >>> analyze_params("SELECT 1 FROM users WHERE id = ANY(%s)", schema_sql=schema)[0].python_type
        'list[int]'
    """
    params = _analyze_params(sql, dialect=dialect, schema_sql=schema_sql)
    return None if params is None else [ParamInfo(*param) for param in params]


def query_diagnostics(
    sql: str, *, dialect: str = "postgres", schema_sql: str | None = None
) -> list[tuple[str, str]]:
    """Why a query has untyped columns, as ``(rule, message)`` pairs.

    The rules are ``unknown-table``, ``unknown-column``, ``ambiguous-column``,
    ``unknown-function``, ``unknown-type`` and ``unsupported-syntax``. Tables and
    columns are only judged when ``schema_sql`` defines some tables.

    Example:
        >>> query_diagnostics("SELECT nope FROM users", schema_sql="CREATE TABLE users (id INT)")
        [('unknown-column', 'column `nope` does not exist in `users`')]
    """
    return _query_diagnostics(sql, dialect=dialect, schema_sql=schema_sql)


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
        ValueError: unsupported ``import_style``, or ``rules`` names an unknown rule or severity
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
    row_usage: RowUsage | None = None,
) -> list[Diagnostic]:
    """Check Python source for SQL type issues.

    A column that can be NULL infers as ``T | None`` (``stale-type`` compares against
    that), and ``nullable-mismatch`` reports a row field that does not admit ``None`` for it.

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

    ``redundant-optional`` reports a row type field only when no query that loads the
    type can return NULL for it. Within ``source`` that is settled here; to settle it
    across files, pass one ``RowUsage`` to every call and drop what its ``filter`` drops
    once all files are checked.

    Example:
        >>> check_source('q = SQL[tuple[str]]("--sql\\nSELECT * FROM users")  # sqlty: ignore[select-star-no-schema]')
        []

    Returns:
        List of diagnostic dicts with keys: rule, message, severity, line

    Raises:
        ValueError: unsupported ``import_style``, or ``rules`` names an unknown rule or severity
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
        row_usage=row_usage or (usage := RowUsage()),
    )
    diagnostics = _apply_suppressions(parsed, diagnostics, rules)
    return diagnostics if row_usage else usage.filter(diagnostics)


class RowUsage:
    """Which fields of the row types a run checks some query needs ``| None`` for.

    ``redundant-optional`` is a question about a row type, not a query: a field is
    redundant only when no query that loads the type can return NULL for it.
    """

    def __init__(self) -> None:
        self.needed: set[tuple[str | None, str, str]] = set()
        self.redundant: list[tuple[tuple[str | None, str, str], Diagnostic]] = []

    def filter(self, diagnostics: list[Diagnostic]) -> list[Diagnostic]:
        """``diagnostics`` without the ``redundant-optional`` reports another query refutes."""
        refuted = {id(diag) for key, diag in self.redundant if key in self.needed}
        return [diag for diag in diagnostics if id(diag) not in refuted]


class _Check:
    """Inputs, derived call sites and the ``report`` sink shared by the check passes."""

    def __init__(
        self,
        parsed: ParsedSource,
        diagnostics: list[Diagnostic],
        *,
        dialect: str,
        schema_sql: str | None,
        rule_config: RuleConfig,
        found: list[HelperCall],
        driver_calls: list[HelperCall],
        queries: list[tuple[int, str, str | None]],
        type_modules: Iterable[str] | None,
        helpers: HelperIndex | None,
        module: str | None,
    ) -> None:
        from sqlty._type_extractor import extract_type_definitions

        self.parsed = parsed
        self.diagnostics = diagnostics
        self.dialect = dialect
        self.schema_sql = schema_sql
        self.rule_config = rule_config
        self.found = found
        self.driver_calls = driver_calls
        self.calls = [call for call in found if call.query is not None]
        self._unparsed: dict[str, str] = {}
        self.rows = [self._unparse(call.row) if call.row else None for call in self.calls]
        named = [
            (call.line, row, call.query)
            for call, row in zip(self.calls, self.rows, strict=True)
            if row
        ]
        self.type_defs: dict[str, TypeInfo] = (
            extract_type_definitions(parsed, helpers, module, type_modules)
            if queries or named
            else {}
        )
        # The row types of the module an imported row type comes from, by local name.
        self.imported_defs: dict[str, dict[str, TypeInfo]] = {}
        self.row_usage = RowUsage()
        if helpers is not None:
            extra = frozenset(type_modules or ())
            for name in {q[1] for q in [*queries, *named]} - self.type_defs.keys():
                if (imported := helpers.row_type(parsed, name, extra)) is not None:
                    self.type_defs[name], self.imported_defs[name] = imported
        self.user_queries = [q for q in [*queries, *named] if q[1] in self.type_defs]

    def _unparse(self, row: ast.expr) -> str:
        """``ast.unparse(row)``, once per distinct source span in the file."""
        if (key := self.parsed.segment(row)) is None:
            return ast.unparse(row)
        if key not in self._unparsed:
            self._unparsed[key] = ast.unparse(row)
        return self._unparsed[key]

    def report(self, rule: str, message: str, line: int) -> Diagnostic | None:
        if not self.rule_config.is_enabled(rule):
            return None
        diagnostic: Diagnostic = {
            "rule": rule,
            "message": message,
            "severity": self.rule_config.severity(rule).value,
            "line": line,
        }
        self.diagnostics.append(diagnostic)
        return diagnostic


def _check_unchecked_queries(ctx: _Check) -> None:
    for call in ctx.found:
        if call.query is None and not _is_any(call):
            ctx.report(
                "unchecked-query",
                "psycopg query is not built from literals only, so it is not checked",
                call.line,
            )


def _check_params(ctx: _Check) -> None:
    if not (
        ctx.rule_config.is_enabled("param-type-mismatch")
        or ctx.rule_config.is_enabled("param-count-mismatch")
    ):
        return
    for call in [*ctx.calls, *ctx.driver_calls]:
        if call.params is not None:
            for rule, message in _param_diagnostics(call, ctx.dialect, ctx.schema_sql):
                ctx.report(rule, message, call.line)


def _check_typeddict_inheritance(ctx: _Check) -> None:
    if not ctx.rule_config.is_enabled("typeddict-inheritance"):
        return
    names = [name for _, name, _ in ctx.user_queries if name not in ctx.imported_defs]
    for info in _typeddict_lineage(names, ctx.type_defs):
        for message in info.errors:
            ctx.report("typeddict-inheritance", message, info.lineno)


def _check_select_star(ctx: _Check) -> None:
    for call in ctx.calls:
        if call.query and (
            found := _select_star_finding(
                call.query, dialect=ctx.dialect, has_schema=ctx.schema_sql is not None
            )
        ):
            ctx.report(*found, call.line)


def _check_unparsed(ctx: _Check) -> None:
    if not ctx.rule_config.is_enabled("unparsed-query"):
        return
    for call in ctx.calls:
        if (
            call.query is not None
            and not _MARKER.match(call.query)
            and (error := _parse_error(call.query, dialect=ctx.dialect))
        ):
            ctx.report("unparsed-query", f"query does not parse: {error}", call.line)


def _check_names(ctx: _Check) -> None:
    if not any(ctx.rule_config.is_enabled(name) for name in NAME_RULES):
        return
    for call in ctx.calls:
        if call.query is None or _MARKER.match(call.query):
            continue
        for rule, message in _query_diagnostics(
            call.query, dialect=ctx.dialect, schema_sql=ctx.schema_sql
        ):
            ctx.report(rule, message, call.line)


def _check_structural(ctx: _Check) -> None:
    """Rows named by a type defined in this source (or an imported one)."""
    for line, name, query in ctx.user_queries:
        if (columns := _columns(query, ctx.dialect, ctx.schema_sql)) is None:
            continue
        info = ctx.type_defs[name]
        defs = ctx.imported_defs.get(name, ctx.type_defs)
        # A field this query can fill with NULL needs `| None`, whatever other queries do.
        pairs = (
            zip((field for field, _ in info.fields), columns, strict=False)
            if info.kind == "NamedTuple"
            else ((info.field_for(col.name), col) for col in columns)
        )
        for field, col in pairs:
            if field is not None and col.nullable is not False:
                ctx.row_usage.needed.add((info.module, info.name, field))
        for rule, message in _structural_diagnostics(name, info, columns, ctx.rule_config, defs):
            diagnostic = ctx.report(rule, message, line)
            if diagnostic is not None and rule == "redundant-optional":
                field = message.split(" is ", 1)[0].removeprefix(f"{name}.")
                ctx.row_usage.redundant.append(((info.module, info.name, field), diagnostic))


def _check_values(ctx: _Check) -> None:
    """Rows spelled as a tuple, ``list[...]`` etc. rather than a named type."""
    shapes: dict[str, _RowShape | None] = {}
    for call, row in zip(ctx.calls, ctx.rows, strict=True):
        if call.row is None or row is None or row in ctx.type_defs:
            continue
        if (columns := _columns(call.query, ctx.dialect, ctx.schema_sql)) is None:
            continue
        if row not in shapes:
            shapes[row] = _row_shape(call.row, row, call.typing_aliases)
        for rule, message in _value_diagnostics(
            shapes[row], columns, ctx.rule_config, ctx.type_defs
        ):
            ctx.report(rule, message, call.line)


_CHECK_PASSES = (
    _check_unchecked_queries,
    _check_params,
    _check_typeddict_inheritance,
    _check_select_star,
    _check_unparsed,
    _check_names,
    _check_structural,
    _check_values,
)


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
    row_usage: RowUsage | None = None,
) -> list[Diagnostic]:
    """Every diagnostic before inline suppression: the Rust pass, then the Python passes."""
    from sqlty._lint import RuleConfig

    diagnostics: list[Diagnostic] = []
    if parsed.may_need_rust_check(import_style):
        diagnostics = cast(
            "list[Diagnostic]",
            _check_source(
                parsed.text,
                dialect=dialect,
                schema_sql=schema_sql,
                rules=rules,
                import_style=import_style,
                untyped=untyped,
                any_calls=parsed.any_calls(import_style),
            ),
        )
    found = find_helper_calls(parsed, helpers, module)
    driver_calls = find_driver_calls(parsed)
    queries = [
        (line, name, query)
        for line, name, query in (
            _wrapped_queries(parsed.text, import_style=import_style)
            if parsed.may_wrap(import_style)
            else []
        )
        if not name.startswith("tuple[")
    ]
    if not (queries or found or driver_calls):
        return diagnostics
    ctx = _Check(
        parsed,
        diagnostics,
        dialect=dialect,
        schema_sql=schema_sql,
        rule_config=RuleConfig(overrides=rules),
        found=found,
        driver_calls=driver_calls,
        queries=queries,
        type_modules=type_modules,
        helpers=helpers,
        module=module,
    )
    ctx.row_usage = row_usage or RowUsage()
    for check_pass in _CHECK_PASSES:
        check_pass(ctx)
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
    for _, kind, text in _query_sites(
        parsed.text, import_style=import_style, any_calls=parsed.any_calls(import_style)
    ):
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
    """Whether the row type is ``typing.Any``: an explicit opt-out of checks."""
    return call.row is not None and is_any(call.row, call.typing_aliases)


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


def _param_diagnostics(
    call: HelperCall, dialect: str, schema_sql: str | None
) -> list[tuple[str, str]]:
    """``(rule, message)`` for the arguments a call passes for its query's placeholders."""
    assert call.query is not None and call.params is not None
    params = analyze_params(call.query, dialect=dialect, schema_sql=schema_sql)
    if params is None:
        return []
    named = bool(params) and all(p.name is not None for p in params)
    positional = all(p.name is None for p in params)
    passed = {arg.key: arg for arg in call.params}
    by_key: dict[int | str, ParamInfo] = {
        (p.name if named and p.name else p.number): p for p in params
    }
    by_name = bool(passed) and all(isinstance(key, str) for key in passed)
    if (named and passed and not by_name) or (positional and by_name):
        return []
    found: list[tuple[str, str]] = []
    if positional and len(passed) != len(by_key):
        found.append(
            (
                "param-count-mismatch",
                f"query has {len(by_key)} parameter{'' if len(by_key) == 1 else 's'}, "
                f"the call passes {len(passed)}",
            )
        )
    elif named and (missing := [f"'{key}'" for key in by_key if key not in passed]):
        found.append(
            (
                "param-count-mismatch",
                f"call is missing parameter{'' if len(missing) == 1 else 's'} {', '.join(missing)}",
            )
        )
    # sqlite3 is dynamically typed, and a cast converts whatever it is given.
    typed = dialect.lower() != "sqlite"
    for key, arg in passed.items():
        if (param := by_key.get(key)) is None:
            continue
        label = f"parameter {key}" if isinstance(key, int) else f"parameter '{key}'"
        if arg.type == "None" or (typed and not param.cast):
            if problem := mismatch(param.python_type, param.nullable, arg.type):
                found.append(("param-type-mismatch", f"{label} {problem}"))
    return found


def _columns(query: str | None, dialect: str, schema_sql: str | None) -> list[_RowColumn] | None:
    """Columns of a query and whether each can be NULL; ``None`` when it has none or cannot be analysed."""
    if query is None:
        return None
    try:
        columns = _analyze_columns(query, dialect=dialect, schema_sql=schema_sql)
    except RuntimeError:
        return None
    return [_RowColumn(*column) for column in columns] if columns else None


def _nullability_diagnostics(
    label: str,
    declared: typing.Any,
    col: _RowColumn,
    rules: RuleConfig,
) -> list[tuple[str, str]]:
    """``nullable-mismatch`` and ``redundant-optional`` for one field and its column.

    ``label`` reads ``<field>`` or ``<row> item <i>``, and is followed by ``is <annotation>``;
    ``declared`` is the resolved annotation. A column of unknown nullability, or an
    annotation that can't be resolved, is never reported.
    """
    from sqlty._type_extractor import admits_none, is_optional
    from sqlty._type_extractor.subtype import _type_repr

    if col.nullable is None or declared is None:
        return []
    if col.nullable and rules.is_enabled("nullable-mismatch") and admits_none(declared) is False:
        return [
            (
                "nullable-mismatch",
                f"{label} is {_type_repr(declared)} but query column '{col.name}' can be NULL",
            )
        ]
    if not col.nullable and rules.is_enabled("redundant-optional") and is_optional(declared):
        return [
            (
                "redundant-optional",
                f"{label} is {_type_repr(declared)} but query column '{col.name}' is never NULL",
            )
        ]
    return []


def _json_diagnostics(
    label: str,
    annotation: ast.expr | None,
    col: _RowColumn,
    type_defs: Mapping[str, TypeInfo],
    aliases: Mapping[str, str],
    rules: RuleConfig,
) -> list[tuple[str, str]]:
    """``json-shape-mismatch`` for one field and a JSON column of known shape.

    ``label`` reads ``<field>`` or ``<row> item <i>`` and is followed by ``is <annotation>``.
    An annotation that can't be read is never reported.
    """
    from sqlty._json_shape import json_problems, summarize

    if col.json_shape is None or annotation is None or not rules.is_enabled("json-shape-mismatch"):
        return []
    problems = json_problems(col.json_shape, annotation, type_defs, aliases, col.name)
    if not problems:
        return []
    return [
        (
            "json-shape-mismatch",
            f"{label} is {ast.unparse(annotation)} but query column '{col.name}' builds JSON "
            f"that does not fit: {summarize(problems)}",
        )
    ]


def _literal_diagnostics(
    label: str,
    declared: typing.Any,
    col: _RowColumn,
    rules: RuleConfig,
) -> list[tuple[str, str]]:
    """``literal-mismatch`` and ``redundant-literal`` for one field and its column.

    Reported only when the annotation is made of ``Literal`` members (and ``None``) and the
    column's values are known: ``str``, ``int``, ``Any`` or an enum take any value, so
    there is nothing to compare.
    """
    from sqlty._type_extractor import column_literal_values, declared_literal_values
    from sqlty._type_extractor.subtype import _type_repr

    if declared is None or not col.python_type:
        return []
    inferred = column_literal_values(col.python_type)
    if inferred is None:
        return []
    admitted = declared_literal_values(declared, inferred)
    if admitted is None:
        return []
    missing = [v for v in inferred if v not in admitted]
    if missing and rules.is_enabled("literal-mismatch"):
        return [
            (
                "literal-mismatch",
                f"{label} is {_type_repr(declared)} but query column '{col.name}' "
                f"can be {', '.join(map(repr, missing))}",
            )
        ]
    extra = sorted((v for v in admitted if v not in inferred), key=repr)
    if not missing and extra and rules.is_enabled("redundant-literal"):
        return [
            (
                "redundant-literal",
                f"{label} is {_type_repr(declared)} but query column '{col.name}' "
                f"is never {', '.join(map(repr, extra))}",
            )
        ]
    return []


def _field_diagnostics(
    label: str,
    declared: typing.Any,
    col: _RowColumn,
    rules: RuleConfig,
) -> list[tuple[str, str]]:
    """Nullability and value-set diagnostics for one field and its column."""
    return _nullability_diagnostics(label, declared, col, rules) + _literal_diagnostics(
        label, declared, col, rules
    )


class _RowShape(NamedTuple):
    """The row type of helper calls, resolved once per file for every call that spells it."""

    text: str
    is_tuple: bool
    targets: list[tuple[str, str, typing.Any, ast.expr]]
    accepts: dict[tuple[int, str], bool]
    aliases: Mapping[str, str]


def _row_shape(row: ast.expr, text: str, aliases: dict[str, str]) -> _RowShape | None:
    """What each column of a helper call's row type ``text`` must be; ``None`` when unchecked.

    A scalar type takes one column, ``tuple[A, B]`` one column per item. Other
    row types (an unknown class, ``dict``, ``tuple[int, ...]``) are not checked.
    """
    from sqlty._type_extractor.ast_fields import _ast_type

    if isinstance(row, ast.Subscript) and _ast_type(row.value, aliases) is tuple:
        items = list(row.slice.elts) if isinstance(row.slice, ast.Tuple) else [row.slice]
        if not items or any(isinstance(i, ast.Constant) and i.value is ... for i in items):
            return None
        annotations = [ast.unparse(item) for item in items]
        targets = [
            (f"{text} item {i} is {annotation}", annotation, _ast_type(item, aliases), item)
            for i, (item, annotation) in enumerate(zip(items, annotations, strict=True))
        ]
        return _RowShape(text, True, targets, {}, aliases)
    resolved = _ast_type(row, aliases)
    if resolved is None or resolved in _CONTAINERS or typing.get_args(resolved):
        return None
    return _RowShape(text, False, [(f"{text} is expected", text, resolved, row)], {}, aliases)


def _value_diagnostics(
    shape: _RowShape | None,
    columns: list[_RowColumn],
    rules: RuleConfig,
    type_defs: Mapping[str, TypeInfo],
) -> list[tuple[str, str]]:
    """Compare the row type of a helper call with the query columns."""
    from sqlty._type_extractor.compat import _accepts

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
    for i, ((label, annotation, resolved, node), col) in enumerate(
        zip(shape.targets, columns, strict=True)
    ):
        item = f"{shape.text} item {i}" if shape.is_tuple else "row type"
        found.extend(_json_diagnostics(item, node, col, type_defs, shape.aliases, rules))
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
        else:
            found.extend(_field_diagnostics(item, resolved, col, rules))
    return found


def _apply_suppressions(
    parsed: ParsedSource,
    diagnostics: list[Diagnostic],
    rules: dict[str, str] | None,
) -> list[Diagnostic]:
    """Drop diagnostics suppressed by a comment on their line; add ``unused-ignore``."""
    from sqlty._lint import RuleConfig

    ignores = parsed.ignores
    if not ignores:
        return diagnostics

    kept: list[Diagnostic] = []
    used: set[tuple[int, str | None]] = set()
    for diag in diagnostics:
        ignore = ignores.get(diag["line"])
        if ignore is None or (ignore.rules is not None and diag["rule"] not in ignore.rules):
            kept.append(diag)
        else:
            used.add((ignore.line, None if ignore.rules is None else diag["rule"]))

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
    type_name: str,
    info: TypeInfo,
    columns: list[_RowColumn],
    rules: RuleConfig,
    type_defs: Mapping[str, TypeInfo],
) -> list[tuple[str, str]]:
    """Compare a user type with query columns, returning ``(rule, message)`` pairs.

    TypedDicts, dataclasses and Pydantic models match columns to fields by
    name, as ``dict_row`` and ``class_row`` pass them: fields with a default
    (``NotRequired`` keys) may be absent, and a column without a field is a
    violation, unless a Pydantic model allows it (``extra="allow"``) or
    ``extra_items`` admits it (type-checked against it). Key-set violations are
    ``typeddict-key-mismatch``. NamedTuples match by position.
    A reported count mismatch suppresses the per-column checks. A column that builds JSON
    of a known shape is also checked against the field's annotation, nested types looked up
    in ``type_defs`` (``json-shape-mismatch``).
    """
    from sqlty._type_extractor import check_type_compatibility, field_type

    fields = info.fields
    declared = dict(fields)
    by_name = info.kind != "NamedTuple"
    by_alias = info.kind == "BaseModel"
    unit = "keys" if info.kind == "TypedDict" else "fields"
    lo = len(info.required - info.opaque) if by_name else len(fields)
    hi = len(columns) if info.extra_items or by_alias else len(fields)
    if not lo <= len(columns) <= hi and rules.is_enabled("column-count-mismatch"):
        expected = f"{len(fields)} fields"
        if info.extra_items or by_alias:
            expected = f"{lo} required {unit}"
        elif lo < hi:
            expected = f"{lo} required and {hi} total {unit}"
        return [
            (
                "column-count-mismatch",
                f"{type_name} has {expected} but query returns {len(columns)} columns",
            )
        ]

    def type_mismatch(field: str, col: _RowColumn) -> list[tuple[str, str]]:
        declared_type = declared.get(field) or f"extra_items {info.extra_items}"
        if col.python_type and not check_type_compatibility(info, field, col.python_type):
            return [
                (
                    "column-type-mismatch",
                    f"{type_name}.{field} is {declared_type} but query column "
                    f"'{col.name}' infers {col.python_type}",
                )
            ]
        return _field_diagnostics(
            f"{type_name}.{field}", field_type(info, field), col, rules
        ) + _json_diagnostics(
            f"{type_name}.{field}", info.annotations.get(field), col, type_defs, info.aliases, rules
        )

    if not by_name:
        pairs = list(zip(fields, columns, strict=False))
        return [d for (field, _), col in pairs for d in type_mismatch(field, col)] + [
            ("column-name-mismatch", f"{type_name}.{field} doesn't match query column '{col.name}'")
            for (field, _), col in pairs
            if field != col.name
        ]

    admits_extra = info.extra_items or info.allows_extra or info.opaque
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
    "RowUsage",
    "analyze_select",
    "check_source",
    "expand_star",
    "fix_source",
    "infer_type",
    "json_shapes",
    "query_diagnostics",
    "returns_rows",
    "rewrite_source",
    "schema_skipped_objects",
    "schema_skipped_tables",
    "strip_source",
]

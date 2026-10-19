"""The SQL engine behind the CLI, implemented in Rust (``sqlty._core``).

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

``check_source`` adds the checks of user-defined row types (TypedDict,
NamedTuple, dataclass, Pydantic) on top of the engine's, since resolving them
needs the Python runtime; those passes live in ``sqlty._check``.
"""

from __future__ import annotations

from collections import Counter
from collections.abc import Iterable, Mapping
from typing import TYPE_CHECKING, NamedTuple

from sqlty._check import RowUsage, apply_suppressions, is_any_row
from sqlty._check import diagnostics as check_diagnostics
from sqlty._core import (
    Schema,
    expand_star,
    infer_type,
    returns_rows,
)
from sqlty._core import analyze_columns as _analyze_columns
from sqlty._core import analyze_params as _analyze_params
from sqlty._core import analyze_select as _analyze_select
from sqlty._core import explain_select as _explain_select
from sqlty._core import fix_source as _fix_source
from sqlty._core import load_schema as _load_schema
from sqlty._core import parse_error as _parse_error
from sqlty._core import query_diagnostics as _query_diagnostics
from sqlty._core import query_sites as _query_sites
from sqlty._core import rewrite_source as _rewrite_source
from sqlty._core import schema_skipped_objects as _schema_skipped_objects
from sqlty._core import schema_skipped_tables as _schema_skipped_tables
from sqlty._core import strip_source as _strip_source
from sqlty._drivers import MARKER
from sqlty._helpers import HelperIndex, find_helper_calls
from sqlty._lint import Diagnostic
from sqlty._source import ParsedSource

if TYPE_CHECKING:
    pass


def load_schema(
    schema_sql: str, *, dialect: str = "postgres", types: Mapping[str, str] | None = None
) -> Schema:
    """Parse ``schema_sql`` once, for passing as ``schema_sql`` to the other functions.

    ``types`` maps a SQL type name or a ``table.column`` to the Python type a converter
    above the driver gives it, as ``[tool.sqlty.types]`` does.

    Example:
        >>> schema = load_schema("CREATE TABLE t (d DATE)", dialect="sqlite", types={"DATE": "datetime.date"})
        >>> infer_type("SELECT d FROM t", dialect="sqlite", schema_sql=schema)
        'tuple[datetime.date | None]'

    Raises:
        ValueError: a key of ``types`` is neither a SQL type nor ``table.column``
    """
    return _load_schema(schema_sql, dialect=dialect, types=list(types.items()) if types else None)


class ColumnInfo(NamedTuple):
    """A result column; types are ``None`` when unknown."""

    name: str
    python_type: str | None
    sql_type: str | None


def analyze_select(
    sql: str, *, dialect: str = "postgres", schema_sql: str | Schema | None = None
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
    sql: str, *, dialect: str = "postgres", schema_sql: str | Schema | None = None
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


class InferenceRule(NamedTuple):
    """An inference rule of the engine: ``rust/crates/sqlty-sql/rules/``."""

    id: str
    reference: str
    reasoning: str


class ColumnRules(NamedTuple):
    """The inference rules behind a result column's type and nullability."""

    name: str
    type_rule: InferenceRule | None
    nullability_rule: InferenceRule | None


def explain_select(
    sql: str, *, dialect: str = "postgres", schema_sql: str | Schema | None = None
) -> list[ColumnRules] | None:
    """The inference rules behind each column of ``analyze_select``, or ``None`` when the
    query returns no rows or doesn't parse.

    A column gets the rules of its outermost operator or function call; a column
    reference, a cast or a set operation has none. ``sqlty check --explain`` prints them.

    Example:
        >>> [rules] = explain_select("SELECT 1::SMALLINT + 1::SMALLINT AS total")
        >>> rules.type_rule.id
        'common.arithmetic.numbers'
        >>> explain_select("SELECT 1::INTEGER AS one")
        [ColumnRules(name='one', type_rule=None, nullability_rule=None)]
    """
    columns = _explain_select(sql, dialect=dialect, schema_sql=schema_sql)
    if columns is None:
        return None
    return [
        ColumnRules(
            name,
            InferenceRule(*type_rule) if type_rule else None,
            InferenceRule(*nullability_rule) if nullability_rule else None,
        )
        for name, type_rule, nullability_rule in columns
    ]


class ParamInfo(NamedTuple):
    """A query parameter. ``name`` is set for ``%(name)s`` and ``:name``; types are ``None`` when unknown.

    ``nullable`` is ``False`` when ``None`` cannot be passed (the parameter is stored in a
    ``NOT NULL`` column), ``True`` when it can, ``None`` when the query does not say.
    ``cast`` is ``True`` when the type is that of an explicit cast (``%s::int``), which
    accepts any value. ``untyped_sql_type`` is the type PostgreSQL gives the parameter when
    the driver sends it untyped, as asyncpg does, when that differs from ``sql_type``.
    """

    number: int
    name: str | None
    python_type: str | None
    sql_type: str | None
    nullable: bool | None
    cast: bool = False
    untyped_sql_type: str | None = None


def analyze_params(
    sql: str, *, dialect: str = "postgres", schema_sql: str | Schema | None = None
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
        [ParamInfo(number=1, name=None, python_type='int', sql_type='INT', nullable=None, cast=False, untyped_sql_type=None), ParamInfo(number=2, name='min', python_type='int', sql_type='INT', nullable=None, cast=False, untyped_sql_type=None)]
        >>> [(p.python_type, p.nullable) for p in analyze_params("INSERT INTO users (id, name, age) VALUES ($1, $2, $3)", schema_sql=schema)]
        [('int', False), ('str', False), ('int', True)]
        >>> analyze_params("SELECT 1 FROM users WHERE id = ANY(%s)", schema_sql=schema)[0].python_type
        'list[int]'
    """
    params = _analyze_params(sql, dialect=dialect, schema_sql=schema_sql)
    return None if params is None else [ParamInfo(*param) for param in params]


def query_diagnostics(
    sql: str, *, dialect: str = "postgres", schema_sql: str | Schema | None = None
) -> list[tuple[str, str]]:
    """Why a query has untyped columns, or names what it cannot, as ``(rule, message)`` pairs.

    The rules are ``unknown-table``, ``unknown-column``, ``ambiguous-column``,
    ``unknown-function``, ``unknown-type``, ``unsupported-syntax``,
    ``insert-missing-column``, ``insert-value-count`` and ``ungrouped-column``. Tables and columns are only judged
    when ``schema_sql``
    defines some tables.

    Example:
        >>> query_diagnostics("SELECT nope FROM users", schema_sql="CREATE TABLE users (id INT)")
        [('unknown-column', 'column `nope` does not exist in `users`')]
    """
    return _query_diagnostics(sql, dialect=dialect, schema_sql=schema_sql)


def schema_skipped_tables(schema_sql: str | Schema, *, dialect: str = "postgres") -> list[str]:
    """Names of schema tables that can't be parsed, so their columns are unknown.

    A table is retried without its constraints and ``CHECK`` / ``REFERENCES``
    clauses first; only tables that still fail are listed.

    Example:
        >>> schema_skipped_tables("CREATE TABLE ok (id INT); CREATE TABLE bad (id INT DEFAULT %s)")
        ['bad']
    """
    return _schema_skipped_tables(schema_sql, dialect=dialect)


def schema_skipped_objects(
    schema_sql: str | Schema, *, dialect: str = "postgres"
) -> list[tuple[str, str]]:
    """``(kind, name)`` of schema tables and views that can't be parsed, so their columns are unknown.

    ``kind`` is ``"table"`` or ``"view"``. Unparseable tables are retried without
    their constraints first, as in :func:`schema_skipped_tables`.

    Example:
        >>> schema_skipped_objects("CREATE TABLE ok (id INT); CREATE VIEW v AS SELECT id FROM (")
        [('view', 'v')]
        >>> schema_skipped_objects("CREATE VIRTUAL TABLE words USING spellfix1", dialect="sqlite")
        [('table', 'words')]
    """
    return _schema_skipped_objects(schema_sql, dialect=dialect)


def rewrite_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    keep_lines: bool = False,
    json_types: str = "typing.Any",
) -> str:
    r"""Wrap each ``--sql`` string in ``SQL[T](...)``, ``T`` the row type it infers.

    With ``json_types="typing.TypedDict"``, a JSON column of known shape gets a generated
    ``TypedDict`` instead of ``typing.Any``.

    Example:
        >>> print(rewrite_source('q = "--sql\\nSELECT 1::INTEGER AS n"'))
        from sqlty import SQL
        q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")

    Raises:
        ValueError: unsupported ``import_style``
    """
    rewritten = _rewrite_source(
        source,
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=import_style,
        untyped=untyped,
        keep_lines=keep_lines,
    )
    if json_types == "typing.TypedDict" and rewritten != source:
        from sqlty._json_types import add_json_types

        rewritten = add_json_types(
            rewritten, dialect=dialect, schema_sql=schema_sql, import_style=import_style
        )
    return rewritten


def strip_source(source: str, *, import_style: str | None = None) -> str:
    r"""Unwrap ``SQL[T](...)`` back to the string, and drop the ``TypedDict``s rewrite generated.

    Example:
        >>> strip_source('from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1")')
        'q = "--sql\\nSELECT 1"'

    Raises:
        ValueError: unsupported ``import_style``
    """
    from sqlty._json_types import strip_json_types

    return _strip_source(strip_json_types(source), import_style=import_style)


def fix_source(
    source: str | ParsedSource,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    rules: dict[str, str] | None = None,
    json_types: str = "typing.Any",
    helpers: HelperIndex | None = None,
    module: str | None = None,
) -> str:
    """Apply what ``sqlty check --fix`` fixes; rules set to ``"ignore"`` are skipped.

    A fix for a rule is not applied to a query whose reported line carries
    ``# sqlty: ignore`` or ``# sqlty: ignore[rule]`` naming that rule, so a
    suppressed ``select-star`` is not expanded. Other rules on the line are fixed.
    A string passed to a helper with its row type (see ``check_source``) is not wrapped.

    Example:
        >>> fix_source('q = "--sql SELECT 1"  # sqlty: ignore[inline-sql-comment]')
        'q = "--sql SELECT 1"  # sqlty: ignore[inline-sql-comment]'

    Raises:
        ValueError: unsupported ``import_style``, or ``rules`` names an unknown rule or severity
    """
    parsed = ParsedSource.of(source, module)
    suppressed: dict[int, list[str] | None] = {
        line: None if ignore.rules is None else list(ignore.rules)
        for line, ignore in parsed.ignores.items()
    }
    for call in find_helper_calls(parsed, helpers, module):
        for line in call.typed_lines:
            if line not in suppressed:
                suppressed[line] = ["stale-rewrite"]
            elif (named := suppressed[line]) is not None:
                named.append("stale-rewrite")
    fixed = _fix_source(
        parsed.text,
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=import_style,
        untyped=untyped,
        rules=rules,
        suppressed=suppressed or None,
    )
    if json_types == "typing.TypedDict" and fixed != parsed.text:
        from sqlty._json_types import add_json_types

        fixed = add_json_types(
            fixed, dialect=dialect, schema_sql=schema_sql, import_style=import_style
        )
    return fixed


def check_source(
    source: str | ParsedSource,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    helpers: HelperIndex | None = None,
    module: str | None = None,
    type_modules: Iterable[str] | None = None,
    row_usage: RowUsage | None = None,
) -> list[Diagnostic]:
    """Check Python source for SQL type issues.

    A column that can be NULL infers as ``T | None`` (``narrow-type`` compares against
    that), and ``nullable-mismatch`` reports a row field that does not admit ``None`` for it.

    When ``SQL[UserType](...)`` names a type defined in the same source,
    structural checks (column count, type, name, TypedDict keys) run instead
    of ``stale-type`` and ``narrow-type``.

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
    diagnostics = check_diagnostics(
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
    diagnostics = apply_suppressions(parsed, diagnostics, rules)
    return diagnostics if row_usage else usage.filter(diagnostics)


CODE_RULES = ("stale-rewrite", "unused-ignore")


def breaking_source(
    source: str | ParsedSource,
    *,
    schema_sql: str | Schema | None,
    against_sql: str | Schema | None,
    dialect: str = "postgres",
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    helpers: HelperIndex | None = None,
    module: str | None = None,
    type_modules: Iterable[str] | None = None,
) -> list[Diagnostic]:
    """The diagnostics ``source`` gets against ``against_sql`` but not against its own
    ``schema_sql``: what a schema change breaks, as ``sqlty breaking`` reports it.

    Both checks apply ``rules`` as ``check_source`` does, so with the default severities a
    dropped or renamed table or column, and a new ``NOT NULL`` column an ``INSERT``
    leaves out, are reported, besides the row type checks; a rule set to ``"ignore"``
    stays off. An annotation wider than the
    other schema's rows (``str | None`` for a ``NOT NULL`` column) still reads them, so it
    is not reported. Nor are ``CODE_RULES``, which judge the code rather than the schema.

    Example:
        >>> old = "CREATE TABLE users (id INT NOT NULL, bio TEXT)"
        >>> new = "CREATE TABLE users (id INT NOT NULL)"
        >>> [d["message"] for d in breaking_source('q = "--sql\\\\nSELECT bio FROM users"', schema_sql=old, against_sql=new)]
        ['column `bio` does not exist in `users`']
    """
    parsed = ParsedSource.of(source, module)

    def check(schema: str | Schema | None) -> list[Diagnostic]:
        return check_source(
            parsed,
            dialect=dialect,
            schema_sql=schema,
            rules=rules,
            import_style=import_style,
            untyped=untyped,
            helpers=helpers,
            module=module,
            type_modules=type_modules,
        )

    own = {(d.get("line"), d["rule"], d["message"]) for d in check(schema_sql)}
    return [
        d
        for d in check(against_sql)
        if (d.get("line"), d["rule"], d["message"]) not in own
        and d["rule"] not in CODE_RULES
        # An annotation wider than the rows still reads them.
        and d["rule"] not in {"stale-type", "redundant-optional", "redundant-literal"}
    ]


def query_statistics(
    source: str | ParsedSource,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
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
            if not is_any_row(call):
                counts["unchecked-query"] += 1
        elif not MARKER.match(call.query) and (
            outcome := _outcome(call.query, dialect, schema_sql)
        ):
            counts[outcome] += 1
    return counts


def _outcome(query: str, dialect: str, schema_sql: str | Schema | None) -> str | None:
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


__all__ = [
    "ColumnInfo",
    "HelperIndex",
    "Schema",
    "RowUsage",
    "analyze_select",
    "check_source",
    "expand_star",
    "fix_source",
    "infer_type",
    "json_shapes",
    "load_schema",
    "query_diagnostics",
    "returns_rows",
    "rewrite_source",
    "schema_skipped_objects",
    "schema_skipped_tables",
    "strip_source",
]

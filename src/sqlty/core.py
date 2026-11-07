"""The SQL engine behind the CLI, implemented in Rust (``sqlty._core``).

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

``check_source`` adds the checks of user-defined row types (TypedDict,
NamedTuple, dataclass, Pydantic) on top of the engine's, since resolving them
needs the Python runtime; those passes live in ``sqlty._check``.
"""

from __future__ import annotations

import ast
from collections import Counter
from collections.abc import Mapping
from typing import TYPE_CHECKING, NamedTuple

from sqlty._check import RowUsage, apply_suppressions, is_any_row
from sqlty._check import diagnostics as check_diagnostics
from sqlty._core import (
    QueryAt,
    Schema,
    expand_star,
    infer_type,
    returns_rows,
)
from sqlty._core import add_param_types as _add_param_types
from sqlty._core import analyze_columns as _analyze_columns
from sqlty._core import analyze_params as _analyze_params
from sqlty._core import analyze_select as _analyze_select
from sqlty._core import complete_source as _complete_source
from sqlty._core import explain_select as _explain_select
from sqlty._core import fix_source as _fix_source
from sqlty._core import format_source as _format_source
from sqlty._core import highlight_source as _highlight_source
from sqlty._core import hover_source as _hover_source
from sqlty._core import load_schema as _load_schema
from sqlty._core import parse_error as _parse_error
from sqlty._core import query_at as _query_at
from sqlty._core import query_diagnostics as _query_diagnostics
from sqlty._core import query_sites as _query_sites
from sqlty._core import rewrite_source as _rewrite_source
from sqlty._core import schema_skipped_objects as _schema_skipped_objects
from sqlty._core import schema_skipped_tables as _schema_skipped_tables
from sqlty._core import strip_source as _strip_source
from sqlty._helpers import MARKER, HelperIndex, find_helper_calls
from sqlty._lint import Diagnostic
from sqlty._source import ParsedSource

if TYPE_CHECKING:
    pass


def load_schema(
    schema_sql: str | None,
    *,
    dialect: str = "postgres",
    types: Mapping[str, str] | None = None,
    name: str | None = None,
    databases: Mapping[str, Schema | None] | None = None,
) -> Schema:
    """Parse ``schema_sql`` once, for passing as ``schema_sql`` to the other functions.

    ``types`` maps a SQL type name or a ``table.column`` to the Python type a converter
    above the driver gives it, as ``[tool.sqlty.types]`` does.

    ``databases`` maps the name of each other database of the project to its schema
    (``None`` for one without): a query whose first line names one, ``--sql
    sqlty:db=analytics``, is read against it in its dialect. ``name`` is this database's,
    ``"default"`` unless set; ``schema_sql=None`` loads a database without a schema.

    Example:
        >>> schema = load_schema("CREATE TABLE t (d DATE)", dialect="sqlite", types={"DATE": "datetime.date"})
        >>> infer_type("SELECT d FROM t", dialect="sqlite", schema_sql=schema)
        'tuple[datetime.date | None]'
        >>> events = load_schema("CREATE TABLE events (n INTEGER NOT NULL)", dialect="sqlite")
        >>> project = load_schema(None, databases={"analytics": events})
        >>> infer_type("--sql sqlty:db=analytics\\nSELECT n FROM events", schema_sql=project)
        'tuple[int]'

    Raises:
        ValueError: a key of ``types`` is neither a SQL type nor ``table.column``
    """
    return _load_schema(
        schema_sql or "",
        dialect=dialect,
        types=list(types.items()) if types else None,
        name=name,
        absent=schema_sql is None,
        databases=list(databases.items()) if databases is not None else None,
    )


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
    the driver sends it untyped, as asyncpg does, when that differs from ``sql_type``: the
    ``%s`` of ``%s + INTERVAL '1 day'`` is an ``interval`` then, and any date or time otherwise.
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
    ``insert-missing-column``, ``insert-value-count``, ``ungrouped-column``,
    ``fanout-aggregate``, ``chasm-join``, ``unproven-join`` and ``entity-mismatch``. Tables and columns are only judged
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
    their constraints first, as in :func:`schema_skipped_tables`. A ``"statement"`` is a
    statement that does not parse and would set keys, checks, ``NOT NULL``, defaults or a
    type, named by its text: an ``ALTER TABLE ... ADD CONSTRAINT`` or a ``CREATE TYPE``.

    Example:
        >>> schema_skipped_objects("CREATE TABLE ok (id INT); CREATE VIEW v AS SELECT id FROM (")
        [('view', 'v')]
        >>> schema_skipped_objects("CREATE VIRTUAL TABLE words USING spellfix1", dialect="sqlite")
        [('table', 'words')]
        >>> schema_skipped_objects("CREATE TABLE t (id INT); ALTER TABLE t ADD PRIMARY KEY (id) WITH (fillfactor = 70)")
        [('statement', 'ALTER TABLE t ADD PRIMARY KEY (id) WITH (fillfactor = 70)')]
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
    param_types: bool = False,
) -> str:
    r"""Wrap each ``--sql`` string in ``SQL[T](...)``, ``T`` the row type it infers.

    With ``json_types="typing.TypedDict"``, a JSON column of known shape gets a generated
    ``TypedDict`` instead of ``typing.Any``. With ``param_types``, each ``SQL[R]`` call gets the
    type of its parameters, ``SQL[R, P]``: a ``tuple``, or a generated ``TypedDict`` for named
    parameters.

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
    if param_types:
        rewritten = _add_param_types(
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
    return _strip_source(source, import_style=import_style)


def format_source(
    source: str, *, dialect: str = "postgres", import_style: str | None = None
) -> str:
    r"""Lay out the query strings ``sqlty check`` reads with `sqruff <https://github.com/quarylabs/sqruff>`_.

    Only a query in one plain triple-quoted literal without a backslash is formatted, and
    only its whitespace changes. The marker line, the indentation of the first line and
    the whitespace before the closing quotes are kept.

    Example:
        >>> print(format_source("q = '''--sql\nselect id,name from users'''"))
        q = '''--sql
        select
            id,
            name
        from users'''

    Raises:
        ValueError: unsupported ``import_style``
    """
    return _format_source(source, dialect=dialect, import_style=import_style)


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
    param_types: bool = False,
) -> str:
    """Apply what ``sqlty check --fix`` fixes; rules set to ``"ignore"`` are skipped.

    A fix for a rule is not applied to a query whose reported line carries
    ``# sqlty: ignore`` or ``# sqlty: ignore[rule]`` naming that rule, so a
    suppressed ``select-star`` is not expanded. Other rules on the line are fixed.
    A string passed to a helper with its row type, or to a configured helper, is not wrapped.

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
    if param_types:
        fixed = _add_param_types(
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
    row_usage: RowUsage | None = None,
    query_variants: int | None = None,
    driver_calls: bool = True,
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
    and ``type`` aliases, ``X = int``, classes), read from its source and never imported.
    Row types are read from source only: nothing of the checked code is imported or run.
    Standard-library classes come from typeshed's class graph and Pydantic's types from a
    table, so a ``TypedDict`` field ``EmailStr`` is a ``str`` and a model field ``AnyUrl``
    takes the ``str`` it validates. Any other imported name is unknown.

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

    ``query_variants`` (the ``preview`` key, up to ``max-query-variants``) checks a query
    a helper call builds from names bound in branches once per variant: each path through
    ``if``, ``match`` and conditional expressions gives one. A diagnostic of some variants
    only names the lines that select one: ``(variant: line 11)``. A call with more variants
    is ``dynamic-query`` (``unchecked-query`` for a psycopg composition).

    ``driver_calls`` (the ``driver-calls`` key) checks the parameters of ``execute``-like
    calls on psycopg, sqlite3 and asyncpg connections; off, only the ``sqlty.drivers``
    helpers are.

    Each diagnostic has the byte offsets of what it is about, ``start`` to ``end``: the
    row type, the query, a parameter type, or the ``# sqlty: ignore`` comment. ``fix``
    holds the edits ``sqlty check --fix`` makes for it, as ``(start, end, replacement)``.

    Example:
        >>> check_source('q = SQL[tuple[str]]("--sql\\nSELECT * FROM users")  # sqlty: ignore[select-star-no-schema]')
        []
        >>> source = 'q = SQL[tuple[int]]("--sql\\\\nSELECT 1.5::REAL")'
        >>> [diagnostic] = check_source(source)
        >>> diagnostic["rule"], source[diagnostic["start"] : diagnostic["end"]], diagnostic["fix"]
        ('narrow-type', 'tuple[int]', [(8, 18, 'tuple[float]')])

    Returns:
        List of ``Diagnostic`` dicts with keys: rule, message, severity, line, start, end, fix

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
        row_usage=row_usage or (usage := RowUsage()),
        variants=query_variants,
        receivers=driver_calls,
    )
    diagnostics = apply_suppressions(parsed.ignores, diagnostics, rules)
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
    query_variants: int | None = None,
    driver_calls: bool = True,
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
            query_variants=query_variants,
            driver_calls=driver_calls,
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
    query_variants: int | None = None,
) -> Counter[str]:
    """How the queries of ``source`` fare, as ``sqlty check --statistics`` counts them.

    Counts ``typed`` queries, ``partially typed`` ones (a column's type is
    unknown), ``unparsed`` ones, and skipped strings by rule
    (``fstring-skipped``, ``tstring-skipped``, ``unchecked-query``, ``dynamic-query``). Queries
    that return no rows are not counted. Each variant of a query built in branches
    (``query_variants``, as in ``check_source``) counts as a query.

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
    for call in find_helper_calls(parsed, helpers, module, variants=query_variants):
        if call.query is None:
            if not is_any_row(call):
                counts["dynamic-query" if call.dynamic else "unchecked-query"] += 1
            continue
        for query in [v.query for v in call.variants] or [call.query]:
            if not MARKER.match(query) and (outcome := _outcome(query, dialect, schema_sql)):
                counts[outcome] += 1
    return counts


def query_at(
    source: str | ParsedSource,
    offset: int,
    *,
    import_style: str | None = None,
    helpers: HelperIndex | None = None,
    module: str | None = None,
) -> QueryAt | None:
    r"""The query ``sqlty check`` reads at byte ``offset`` of ``source``, or ``None`` outside one.

    A string is a query exactly when ``check_source`` checks it: a ``--sql`` marked string,
    the argument of ``SQL[T](...)``, or a string literal passed to a helper's query
    parameter (``helpers`` and ``module`` as for ``check_source``). An f-string, a variable
    or a call to anything else is not. The cursor counts as in the query anywhere between
    the quotes, both ends included, after the marker.

    ``offset`` counts UTF-8 bytes. The result's ``text`` is the query as the analyzer reads
    it (escapes decoded, the marker spelled ``--sql``), ``offset`` the cursor in it, and
    ``source_offset(n)`` maps a byte of ``text`` back to the source.

    Example:
        >>> source = 'q = "--sql\\nSELECT id FROM users"'
        >>> at = query_at(source, source.index("id"))
        >>> at.text, at.offset, at.route
        ('--sql\nSELECT id FROM users', 13, 'marker')
        >>> at.source_offset(at.offset) == source.index("id")
        True
        >>> query_at(source, 0) is None
        True
    """
    parsed = ParsedSource.of(source, module)
    starts = _helper_starts(parsed, helpers, module)
    return _query_at(parsed.text, offset, import_style=import_style, helper_starts=starts)


class Completion(NamedTuple):
    """A name that can be typed at the cursor.

    ``kind`` is ``table``, ``view``, ``cte``, ``column``, or ``join`` for a table a
    foreign key links to one in scope, its ``ON`` clause included; ``detail`` the schema of
    a relation or the alias (or table) of a column.
    """

    label: str
    kind: str
    detail: str | None
    sql_type: str | None
    nullable: bool | None


class Completions(NamedTuple):
    """What completes at a cursor: ``items`` replace the bytes ``start`` to ``end`` of the source."""

    start: int
    end: int
    incomplete: bool
    items: list[Completion]


def complete_source(
    source: str | ParsedSource,
    line: int,
    column: int,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    helpers: HelperIndex | None = None,
    module: str | None = None,
    limit: int = 200,
) -> Completions | None:
    r"""The table and column names that complete at 1-based ``line`` and ``column`` of ``source``.

    ``column`` counts characters, as ``sqlty check --output-format json`` does. The cursor
    must be in a query ``check`` reads (see ``query_at``); elsewhere the result is ``None``.
    In a query, ``items`` lists the relations (after ``FROM``, ``JOIN``, ``UPDATE``,
    ``INTO``) or the columns of the relations in scope (after ``alias.`` and in
    ``SELECT``, ``WHERE``, ``ON``, ...) whose name starts with the word before the cursor,
    at most ``limit``, with ``incomplete`` set when there were more. ``start`` and ``end``
    are the byte offsets of that word, which a completion replaces.

    Example:
        >>> schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT)"
        >>> source = 'q = "--sql\\nSELECT u. FROM users u"'
        >>> found = complete_source(source, 1, source.index("u.") + 3, schema_sql=schema)
        >>> [(c.label, c.kind, c.sql_type, c.nullable) for c in found.items]
        [('id', 'column', 'INT', False), ('name', 'column', 'TEXT', True)]
        >>> complete_source(source, 1, 1, schema_sql=schema) is None
        True
    """
    parsed = ParsedSource.of(source, module)
    found = _complete_source(
        parsed.text,
        parsed.offset(line, column),
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=import_style,
        helper_starts=_helper_starts(parsed, helpers, module),
        limit=limit,
    )
    if found is None:
        return None
    start, end, incomplete, items = found
    return Completions(start, end, incomplete, [Completion(*item) for item in items])


class Hover(NamedTuple):
    """What a word of a query is: ``contents`` in Markdown, for the bytes ``start`` to ``end``."""

    start: int
    end: int
    contents: str


def hover_source(
    source: str | ParsedSource,
    line: int,
    column: int,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    helpers: HelperIndex | None = None,
    module: str | None = None,
) -> Hover | None:
    r"""What the word at 1-based ``line`` and ``column`` of a query string is.

    ``column`` counts characters, as in ``complete_source``. A word at the top level of a
    select item shows its result column: the Python and SQL types and the inference rules
    behind them. Any other name shows the column, table, view or CTE it names, and a word
    before ``.`` the columns of that relation. ``None`` outside a query ``check`` reads or
    on a word that names nothing. ``sqlty server`` answers ``textDocument/hover`` with it.

    Example:
        >>> schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT)"
        >>> source = 'q = "--sql\\nSELECT name FROM users WHERE id = 1"'
        >>> found = hover_source(source, 1, source.index("id =") + 1, schema_sql=schema)
        >>> found.contents
        'Column `id` of `users`: `INT NOT NULL`'
        >>> source.encode()[found.start : found.end]
        b'id'
    """
    parsed = ParsedSource.of(source, module)
    found = _hover_source(
        parsed.text,
        parsed.offset(line, column),
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=import_style,
        helper_starts=_helper_starts(parsed, helpers, module),
    )
    return None if found is None else Hover(*found)


class Highlight(NamedTuple):
    """A token to color: the bytes ``start`` to ``end`` of the source.

    ``kind`` is ``keyword``, ``string``, ``number``, ``comment``, ``operator`` or
    ``parameter``.
    """

    start: int
    end: int
    kind: str


def highlight_source(
    source: str | ParsedSource,
    *,
    dialect: str = "postgres",
    import_style: str | None = None,
    helpers: HelperIndex | None = None,
    module: str | None = None,
) -> list[Highlight]:
    r"""The SQL tokens to color in the strings ``sqlty check`` reads, in order.

    The strings are those of ``query_at``; every other string stays a Python string. Names
    are not colored: a word is a keyword only when it is a reserved word or a clause, so
    ``name`` and ``status`` stay names. A token written across string literals or escape
    sequences gets one range per piece. ``sqlty server`` sends these as semantic tokens.

    Example:
        >>> source = 'q = "--sql\\nSELECT 1"; x = "SELECT 2"'
        >>> [(source[h.start : h.end], h.kind) for h in highlight_source(source)]
        [('--sql', 'comment'), ('SELECT', 'keyword'), ('1', 'number')]
    """
    parsed = ParsedSource.of(source, module)
    found = _highlight_source(
        parsed.text,
        dialect=dialect,
        import_style=import_style,
        helper_starts=_helper_starts(parsed, helpers, module),
    )
    return [Highlight(*item) for item in found]


def _helper_starts(
    parsed: ParsedSource, helpers: HelperIndex | None = None, module: str | None = None
) -> list[int]:
    """The byte offsets of the string literals helper calls pass as their query."""
    return [
        parsed.span(call.query_node)[0]
        for call in find_helper_calls(parsed, helpers, module)
        if isinstance(call.query_node, ast.Constant) and isinstance(call.query_node.value, str)
    ]


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
    "QueryAt",
    "Schema",
    "RowUsage",
    "Completion",
    "Completions",
    "Highlight",
    "Hover",
    "analyze_select",
    "check_source",
    "complete_source",
    "expand_star",
    "fix_source",
    "format_source",
    "highlight_source",
    "hover_source",
    "infer_type",
    "json_shapes",
    "load_schema",
    "query_at",
    "query_diagnostics",
    "returns_rows",
    "rewrite_source",
    "schema_skipped_objects",
    "schema_skipped_tables",
    "strip_source",
]

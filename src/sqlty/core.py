"""The SQL engine behind the CLI, implemented in Rust (``sqlty._core``).

Usage:
    from sqlty.core import infer_type, rewrite_source, strip_source, check_source

``check_source`` adds the checks of user-defined row types (TypedDict,
NamedTuple, dataclass, Pydantic) on top of the engine's, since resolving them
needs the Python runtime; those passes live in ``sqlty._check``.
"""

from __future__ import annotations

import sys
from collections.abc import Collection, Mapping
from typing import TYPE_CHECKING, NamedTuple

from sqlty._check import RowUsage
from sqlty._check import diagnostics as check_diagnostics
from sqlty._core import (
    Schema,
    checks_names,
    expand_star,
    infer_type,
    returns_rows,
)
from sqlty._core import add_json_types as _add_json_types
from sqlty._core import add_param_types as _add_param_types
from sqlty._core import analyze_params as _analyze_params
from sqlty._core import analyze_select as _analyze_select
from sqlty._core import complete_source as _complete_source
from sqlty._core import fix_module as _fix_module
from sqlty._core import format_source as _format_source
from sqlty._core import highlight_source as _highlight_source
from sqlty._core import load_schema as _load_schema
from sqlty._core import module_calls as _module_calls
from sqlty._core import rewrite_source as _rewrite_source
from sqlty._core import schema_skipped_objects as _schema_skipped_objects
from sqlty._core import strip_source as _strip_source
from sqlty._helpers import HelperIndex
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
    """
    columns = _analyze_select(sql, dialect=dialect, schema_sql=schema_sql)
    return None if columns is None else [ColumnInfo(*column) for column in columns]


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


def schema_skipped_objects(
    schema_sql: str | Schema, *, dialect: str = "postgres"
) -> list[tuple[str, str]]:
    """``(kind, name)`` of schema tables and views that can't be parsed, so their columns are unknown.

    ``kind`` is ``"table"`` or ``"view"``. Unparseable tables are retried without
    their constraints and ``CHECK`` / ``REFERENCES`` clauses first. A ``"statement"`` is a
    statement that does not parse and would set keys, checks, ``NOT NULL``, defaults or a
    type, named by its text: an ``ALTER TABLE ... ADD CONSTRAINT`` or a ``CREATE TYPE``.
    A ``"limit"`` is a table, view or statement left unknown because the schema is too
    large: over 4,096 columns in a relation, or far more columns than it spells out.

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
    param_types: bool = True,
    closed_params: bool = False,
) -> str:
    r"""Wrap each ``--sql`` string in ``SQL[T](...)``, ``T`` the row type it infers.

    With ``json_types="typing.TypedDict"``, a JSON column of known shape gets a generated
    ``TypedDict`` instead of ``typing.Any``. With ``param_types``, each ``SQL[R]`` call gets the
    type of its parameters, ``SQL[R, P]``: a ``tuple``, or a generated ``TypedDict`` for named
    parameters, ``closed=True`` with ``closed_params`` (Python 3.15).

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
        rewritten = _json_types(rewritten, dialect, schema_sql, import_style)
    if param_types:
        rewritten = _add_param_types(
            rewritten,
            dialect=dialect,
            schema_sql=schema_sql,
            import_style=import_style,
            closed=closed_params,
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
    param_types: bool = True,
    closed_params: bool = False,
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
    fixed = _fixed(parsed, dialect, schema_sql, import_style, untyped, rules, helpers, module)
    if json_types == "typing.TypedDict" and fixed != parsed.text:
        fixed = _json_types(fixed, dialect, schema_sql, import_style)
    if param_types:
        fixed = _add_param_types(
            fixed,
            dialect=dialect,
            schema_sql=schema_sql,
            import_style=import_style,
            closed=closed_params,
        )
    return fixed


def _fixed(
    parsed: ParsedSource,
    dialect: str,
    schema_sql: str | Schema | None,
    import_style: str | None,
    untyped: bool,
    rules: dict[str, str] | None,
    helpers: HelperIndex | None,
    module: str | None,
) -> str:
    """``fix_source`` before the JSON and parameter types."""
    return _fix_module(
        parsed.text,
        version=sys.version_info[:2],
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=import_style,
        untyped=untyped,
        rules=rules,
        index=None if helpers is None else helpers.rust(),
        name=parsed.module,
        module=module,
    )


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
    coerce_row_types: Collection[str] = (),
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

    ``coerce_row_types`` (the ``coerce-row-types`` key) names the ``TypedDict``,
    ``NamedTuple`` and dataclass rows the code validates with Pydantic, such as through a
    ``TypeAdapter``, as ``module.Name``: their fields take what a ``BaseModel`` field takes.

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
        coerce=frozenset(coerce_row_types),
        suppress=True,
    )
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
    coerce_row_types: Collection[str] = (),
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
            coerce_row_types=coerce_row_types,
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
    must be in a query ``check`` reads: a ``--sql`` marked string, the argument of
    ``SQL[T](...)``, or a string literal passed to a helper's query parameter (``helpers``
    and ``module`` as for ``check_source``); elsewhere the result is ``None``.
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

    The strings are those of ``complete_source``; every other string stays a Python string. Names
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


def _json_types(
    source: str, dialect: str, schema_sql: str | Schema | None, import_style: str | None
) -> str:
    """``source`` with the ``typing.Any`` row items of JSON columns of known shape typed."""
    return _add_json_types(
        source,
        version=sys.version_info[:2],
        dialect=dialect,
        schema_sql=schema_sql,
        import_style=import_style,
    )


def _helper_starts(
    parsed: ParsedSource, helpers: HelperIndex | None = None, module: str | None = None
) -> list[int]:
    """The byte offsets of the string literals helper calls pass as their query."""
    return [start for _, _, _, start in _helper_sites(parsed, helpers, module) if start is not None]


type _Site = tuple[tuple[int, int] | None, list[str], str | None, int | None]
"""A helper call: its first and last line when its strings need no ``SQL[T]``, the queries
it can run that the lexer does not read by their marker, the rule of a call that is not
checked, and the byte offset of the string literal it passes as its query."""


def _helper_sites(
    parsed: ParsedSource,
    helpers: HelperIndex | None,
    module: str | None,
    variants: int | None = None,
) -> list[_Site]:
    return _module_calls(
        parsed.text,
        version=sys.version_info[:2],
        index=None if helpers is None else helpers.rust(),
        name=parsed.module,
        module=module,
        variants=variants,
    )


__all__ = [
    "ColumnInfo",
    "HelperIndex",
    "Schema",
    "RowUsage",
    "Completion",
    "Completions",
    "Highlight",
    "analyze_params",
    "analyze_select",
    "breaking_source",
    "check_source",
    "checks_names",
    "complete_source",
    "expand_star",
    "fix_source",
    "format_source",
    "highlight_source",
    "infer_type",
    "load_schema",
    "returns_rows",
    "rewrite_source",
    "schema_skipped_objects",
    "strip_source",
]

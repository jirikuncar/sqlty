"""
SQL analysis utilities using sqlglot to extract column and type information.
"""

import re
from pathlib import Path
from typing import Any, NamedTuple

from sqlglot import exp, parse
from sqlglot.dialects.dialect import Dialect
from sqlglot.dialects.mysql import MySQL
from sqlglot.dialects.postgres import Postgres
from sqlglot.dialects.sqlite import SQLite
from sqlglot.errors import ParseError
from sqlglot.parser import Parser
from sqlglot.tokens import TokenType

# Module-level default to avoid re-creating per call


class ColumnInfo(NamedTuple):
    """Information about a column in a SQL SELECT statement."""

    name: str
    python_type: str | None
    sql_type: str | None


class SQLAnalyzer:
    """Analyzes SQL queries to extract column information."""

    # Mapping from SQL types to Python types
    SQL_TO_PYTHON_TYPE = {
        "INT": "int",
        "INTEGER": "int",
        "BIGINT": "int",
        "SMALLINT": "int",
        "TINYINT": "int",
        "MEDIUMINT": "int",
        "YEAR": "int",
        "FLOAT": "float",
        "DOUBLE": "float",
        "REAL": "float",
        "DECIMAL": "float",
        "NUMERIC": "float",
        "TEXT": "str",
        "VARCHAR": "str",
        "CHAR": "str",
        "STRING": "str",
        "BOOLEAN": "bool",
        "BOOL": "bool",
        "DATE": "datetime.date",
        "TIMESTAMP": "datetime.datetime",
        "TIMESTAMPTZ": "datetime.datetime",
        "DATETIME": "datetime.datetime",
        "TIME": "datetime.time",
        "TIMETZ": "datetime.time",
        "INTERVAL": "datetime.timedelta",
        "ARRAY": "list",
        "JSON": "dict",
        "JSONB": "dict",
        "UUID": "uuid.UUID",
        "BYTEA": "bytes",
        "BLOB": "bytes",
        "BINARY": "bytes",
        "VARBINARY": "bytes",
        # MySQL ``CAST(x AS SIGNED/UNSIGNED)``
        "SIGNED": "int",
        "UNSIGNED": "int",
        # MySQL ``... UNSIGNED`` columns
        "UTINYINT": "int",
        "USMALLINT": "int",
        "UMEDIUMINT": "int",
        "UINT": "int",
        "UBIGINT": "int",
        "UDOUBLE": "float",
        "UDECIMAL": "float",
    }

    def __init__(self, dialect: str = "postgres", schema_path: Path | None = None) -> None:
        self.dialect = dialect
        self.schema_path = schema_path
        self.schema: dict[str, dict[str, str]] = {}  # table_name -> {column_name -> sql_type}

        if schema_path:
            self._load_schema(schema_path)

    def _load_schema(self, schema_path: Path) -> None:
        """
        Load and parse SQL schema file to extract table and column type information.

        Parses CREATE TABLE statements to build a schema dictionary mapping:
        table_name -> {column_name -> sql_type}

        Raises:
            RuntimeError: If the schema file cannot be read or does not exist.
        """
        if not schema_path.exists():
            raise RuntimeError(f"Schema file not found: {schema_path}")

        try:
            schema_sql = schema_path.read_text()
        except Exception as e:
            raise RuntimeError(f"Failed to read schema file {schema_path}: {e}") from e

        # Parse all statements in the schema file
        for statement in schema_sql.split(";"):
            statement = statement.strip()
            if not statement:
                continue

            try:
                ast = _parse_one(statement, self.dialect)

                # Look for CREATE TABLE statements
                if isinstance(ast, exp.Create) and ast.find(exp.Table):
                    table_expr = ast.find(exp.Table)
                    if table_expr:
                        table_name = table_expr.name.lower()

                        # Extract column definitions
                        schema_def = ast.find(exp.Schema)
                        if schema_def:
                            self.schema[table_name] = {}
                            for col_def in schema_def.expressions:
                                if isinstance(col_def, exp.ColumnDef):
                                    col_name = col_def.name.lower()
                                    # Get the column type
                                    if col_def.kind:
                                        col_type = str(col_def.kind).upper()
                                        self.schema[table_name][col_name] = col_type
            except Exception:
                # Skip statements that can't be parsed
                continue

    def _get_column_type_from_schema(self, table_name: str, column_name: str) -> str | None:
        """
        Look up column type from loaded schema.

        Returns the SQL type if found, None otherwise.
        """
        return self.schema.get(table_name.lower(), {}).get(column_name.lower())

    def returns_rows(self, sql: str) -> bool:
        """Whether ``sql`` yields rows: a query, DML with RETURNING, or unparseable SQL.

        Empty SQL, DDL, DML without RETURNING and transaction control return no rows.

        Example:
            >>> SQLAnalyzer().returns_rows("DELETE FROM users RETURNING id")
            True
            >>> SQLAnalyzer().returns_rows("DELETE FROM users")
            False
        """
        try:
            statements = _parse(_query_input(sql), self.dialect)
        except Exception:
            return True
        statement = statements[0] if statements else None
        if isinstance(statement, _DML):
            return bool(statement.args.get("returning"))
        return isinstance(statement, (exp.Query, exp.Values))

    def analyze_select(self, sql: str) -> list[ColumnInfo] | None:
        """Analyze a SQL query and extract column information from its result set.

        Rows come from the leftmost ``SELECT`` of a query (``UNION`` and
        ``WITH`` included) or from the top-level ``RETURNING`` of
        INSERT/UPDATE/DELETE/MERGE. Anything else returns no rows.

        Example:
            >>> [c.python_type for c in SQLAnalyzer().analyze_select("SELECT COUNT(*) AS n, 'x'")]
            ['int', 'str']
            >>> SQLAnalyzer().analyze_select("INSERT INTO t SELECT a::INT FROM u") is None
            True

        Raises:
            RuntimeError: ``SUM(column)`` names a column missing from a schema table.
        """
        try:
            ast = _parse_one(_query_input(sql), self.dialect)
        except Exception:
            return None

        if isinstance(ast, _DML):
            returning = ast.args.get("returning")
            if not returning or not returning.expressions:
                return None
            target = ast.this.this if isinstance(ast.this, exp.Schema) else ast.this
            scope = _scope([target])
            return self._process_projections(returning.expressions, scope) or None

        select = _leftmost_select(ast)
        if select is None or not select.expressions:
            return None
        return self._process_projections(select.expressions, _select_scope(select), select) or None

    def _process_projections(
        self,
        projections: list[exp.Expression],
        scope: list[tuple[str, str]],
        select: exp.Select | None = None,
    ) -> list[ColumnInfo]:
        """Infer one ``ColumnInfo`` per result column; a star may yield several.

        A star that cannot be resolved from the schema yields one ``*`` column.
        """
        columns: list[ColumnInfo] = []
        for projection in projections:
            expr = projection.this if isinstance(projection, exp.Alias) else projection
            if isinstance(expr, exp.Star) or (
                isinstance(expr, exp.Column) and isinstance(expr.this, exp.Star)
            ):
                table_ref = expr.table if isinstance(expr, exp.Column) else None
                expanded = (
                    self._star_columns(table_ref, scope)
                    if select is None or _star_resolvable(select, table_ref)
                    else None
                )
                columns.extend(expanded or [ColumnInfo("*", None, None)])
                continue
            sql_type, python_type = self._infer_expr(expr, scope)
            columns.append(ColumnInfo(projection.alias_or_name, python_type, sql_type))
        return columns

    def _star_columns(
        self, table_ref: str | None, scope: list[tuple[str, str]]
    ) -> list[ColumnInfo] | None:
        """Expand ``*`` (all scope tables) or ``t.*`` from the schema, or None if unknown."""
        tables = [_resolve(table_ref, scope)] if table_ref else [table for _, table in scope]
        if not tables or any(table not in self.schema for table in tables):
            return None
        return [
            ColumnInfo(name, self._sql_to_python_type(sql_type), sql_type)
            for table in tables
            for name, sql_type in self.schema[table].items()
        ]

    def _column_type(self, column: exp.Column, scope: list[tuple[str, str]]) -> str | None:
        """Schema type of a column: its qualifier's table, else the first scope table having it."""
        if column.table:
            return self._get_column_type_from_schema(_resolve(column.table, scope), column.name)
        for _, table in scope:
            if sql_type := self._get_column_type_from_schema(table, column.name):
                return sql_type
        return None

    def _infer_expr(
        self, expr: exp.Expression, scope: list[tuple[str, str]]
    ) -> tuple[str | None, str | None]:
        """Infer ``(sql_type, python_type)`` for one expression; None marks unknown."""
        match expr:
            case exp.Cast(to=to):
                return str(to), self._sql_to_python_type(str(to))
            case exp.Paren(this=inner) | exp.Window(this=inner) | exp.Coalesce(this=inner):
                return self._infer_expr(inner, scope)
            case exp.Column():
                sql_type = self._column_type(expr, scope)
                return sql_type, self._sql_to_python_type(sql_type) if sql_type else None
            case exp.Literal():
                if expr.is_string:
                    return None, "str"
                return None, "float" if any(c in expr.this for c in ".eE") else "int"
            case exp.Not(this=exp.Exists() | exp.In() as inner):
                return self._infer_expr(inner, scope)
            case exp.Boolean() | exp.Exists():
                return None, "bool"
            case exp.In():
                return None, "bool" if expr.args.get("query") else None
            case exp.Subquery(this=query):
                select = _leftmost_select(query)
                if select is None or not select.expressions:
                    return None, None
                first = self._process_projections(
                    select.expressions[:1], _select_scope(select), select
                )
                return (first[0].sql_type, first[0].python_type) if first else (None, None)
            case exp.Case():
                ifs = expr.args.get("ifs") or []
                branch = ifs[0].args.get("true") if ifs else expr.args.get("default")
                return self._infer_expr(branch, scope) if branch is not None else (None, None)
            case exp.Count():
                return "INTEGER", "int"
            case exp.Avg():
                return None, "float"
            case exp.Sum() | exp.Min() | exp.Max():
                return self._infer_aggregate(expr, scope)
            case exp.Anonymous(this=exp.Identifier(quoted=True)):
                return None, None
            case exp.Func():
                name = expr.name if isinstance(expr, exp.Anonymous) else expr.sql_name()  # type: ignore[no-untyped-call]
                return None, _FUNCTION_TYPES.get(name.upper())
        return None, None

    def _infer_aggregate(
        self, func: exp.Func, scope: list[tuple[str, str]]
    ) -> tuple[str | None, str | None]:
        """SUM/MIN/MAX take their argument's type; SUM falls back to float.

        Raises:
            RuntimeError: ``SUM(column)`` names a column missing from a schema table.
        """
        sql_type, python_type = self._infer_expr(func.this, scope)
        if python_type is not None or not isinstance(func, exp.Sum):
            return sql_type, python_type
        if isinstance(column := func.this, exp.Column) and scope:
            table = _resolve(column.table, scope) if column.table else scope[0][1]
            if table in self.schema:
                raise RuntimeError(
                    f"Column '{column.name}' not found in table '{table}' in schema. "
                    f"If you want to query columns not in the schema, use an explicit cast: "
                    f"SUM({column.name}::TYPE)"
                )
        return "NUMERIC", "float"

    def _sql_to_python_type(self, sql_type: str) -> str:
        """Convert SQL type to Python type."""
        normalized = re.split(r"[(<]", sql_type.upper(), maxsplit=1)[0].strip()
        return self.SQL_TO_PYTHON_TYPE.get(normalized, "str")

    def expand_star(self, sql: str) -> str | None:
        """Expand SELECT * into explicit column list using loaded schema.

        Replaces `SELECT *` or `SELECT t.*` with `SELECT col1::TYPE, col2::TYPE, ...`
        using column definitions from the loaded schema, in declaration order.
        Other projections are kept verbatim.

        Example:
            >>> import tempfile, pathlib
            >>> schema = pathlib.Path(tempfile.mkdtemp()) / "schema.sql"
            >>> _ = schema.write_text("CREATE TABLE users (id INTEGER, name TEXT);")
            >>> SQLAnalyzer(schema_path=schema).expand_star("SELECT *, 1 AS one FROM users")
            'SELECT id::INT, name::TEXT, 1 AS one FROM users'

        Returns:
            The rewritten SQL with explicit columns, or None if:
            - No schema is loaded
            - Query doesn't contain SELECT *
            - Referenced table is not in schema
            - SQL cannot be parsed
        """
        if not self.schema:
            return None
        try:
            select = _leftmost_select(_parse_one(_query_input(sql), self.dialect))
        except Exception:
            return None
        # Expansion drops table qualifiers, so with more than one source it could
        # write ambiguous columns.
        if select is None or len(_relations(select)) != 1:
            return None
        scope = _select_scope(select)

        upper = sql.upper()
        select_idx = _top_level(upper, "SELECT")
        from_idx = _top_level(upper, "FROM", select_idx) if select_idx >= 0 else -1
        if from_idx < 0:
            return None

        items: list[str] = []
        has_star = False
        for item in _split_top_level(sql[select_idx + len("SELECT") : from_idx]):
            star = re.fullmatch(r"(?:(\w+)\s*\.\s*)?\*", item)
            if star is None:
                items.append(item)
                continue
            columns = self._star_columns(star.group(1), scope)
            if columns is None or not _star_resolvable(select, star.group(1)):
                return None
            items.extend(f"{col.name}::{col.sql_type}" for col in columns)
            has_star = True
        if not has_star:
            return None
        return f"{sql[:select_idx]}SELECT {', '.join(items)} {sql[from_idx:]}"

    def generate_return_type(self, sql: str) -> str | None:
        """
        Generate a Python return type annotation for a SQL query.

        Args:
            sql: SQL query string to analyze

        For example:
        - "SELECT id, name FROM users" -> "tuple[int, str]" (if types can be inferred)
        - "SELECT id, name FROM users" -> "tuple[str, str]" (if types can't be inferred)
        """
        columns = self.analyze_select(sql)
        # No columns, or a ``*`` the schema cannot resolve: the row type is unknown.
        if not columns or any(col.name == "*" and col.python_type is None for col in columns):
            return None

        # Build type list
        types: list[str] = []
        for col in columns:
            if col.python_type:
                types.append(col.python_type)
            else:
                # Default to str if type can't be inferred
                types.append("str")

        return f"tuple[{', '.join(types)}]"


_DML = (exp.Insert, exp.Update, exp.Delete, exp.Merge)

# Result types of functions by name; mirrors ``rust/src/analyzer.rs``.
# sqlglot renames a few (STRING_AGG -> GROUP_CONCAT, JSON_AGG -> J_S_O_N_ARRAY_AGG, ...).
_FUNCTION_TYPES = {
    **dict.fromkeys(
        ["LENGTH", "POSITION", "STR_POSITION", "CHAR_LENGTH", "BIT_LENGTH", "OCTET_LENGTH"]
        + ["ROW_NUMBER", "RANK", "DENSE_RANK", "NTILE"],
        "int",
    ),
    **dict.fromkeys(
        ["STDDEV", "STDDEV_POP", "STDDEV_SAMP", "VARIANCE", "VAR_POP", "VAR_SAMP"]
        + ["VARIANCE_POP"],
        "float",
    ),
    **dict.fromkeys(
        ["UPPER", "LOWER", "TRIM", "CONCAT", "SUBSTR", "SUBSTRING", "REPLACE", "STRING_AGG"]
        + ["GROUP_CONCAT"],
        "str",
    ),
    **dict.fromkeys(["NOW", "CURRENT_TIMESTAMP"], "datetime.datetime"),
    "CURRENT_DATE": "datetime.date",
    "CURRENT_TIME": "datetime.time",
    **dict.fromkeys(
        ["JSON_BUILD_OBJECT", "JSONB_BUILD_OBJECT", "TO_JSON", "TO_JSONB", "JSON_AGG", "JSONB_AGG"]
        + ["JSONB_OBJECT_AGG", "JSON_OBJECT_AGG", "J_S_O_N_ARRAY_AGG", "J_S_O_N_B_OBJECT_AGG"]
        + ["J_S_O_N_OBJECT_AGG"],
        "dict",
    ),
}


# The overrides below rely on private sqlglot parser APIs: ``_parse_types``,
# ``_parse_function_call``, ``_parse_number``, ``_tokens``, ``_curr``, ``_index``,
# ``_retreat``, ``_match`` and ``_match_text_seq``, plus the ``schema`` and
# ``anonymous`` keyword arguments. Recheck them when raising the sqlglot upper
# bound in pyproject.toml.

_UNSIGNED_TYPES = {
    **Parser.SIGNED_TO_UNSIGNED_TYPE_TOKEN,
    TokenType.FLOAT: TokenType.UDOUBLE,
}


class _QuotedFunctionNames(Parser):
    """Read a quoted name such as ``"CAST"(x)`` as a plain call, as sqlparser-rs does."""

    def _parse_function_call(self, *args: Any, **kwargs: Any) -> exp.Expression | None:
        if self._curr and self._curr.token_type == TokenType.IDENTIFIER:
            kwargs["anonymous"] = True
        return super()._parse_function_call(*args, **kwargs)


class _MySQL(MySQL):
    """MySQL read the way sqlparser-rs reads it.

    ``INT3``/``MIDDLEINT`` are ``MEDIUMINT`` and ``INT8`` is ``BIGINT``; a trailing
    ``SIGNED`` is accepted; ``FLOAT UNSIGNED`` is ``UDOUBLE``; spatial types are
    user-defined types; ``INT[]``, ``NUMERIC ... UNSIGNED`` and ``FIXED ... UNSIGNED``
    are rejected.
    """

    # MySQL's CAST takes a fixed list of types; accepting any name matches sqlparser-rs.
    SUPPORTS_USER_DEFINED_TYPES = True

    class Tokenizer(MySQL.Tokenizer):
        KEYWORDS = {
            **MySQL.Tokenizer.KEYWORDS,
            "INT3": TokenType.MEDIUMINT,
            "MIDDLEINT": TokenType.MEDIUMINT,
            "INT8": TokenType.BIGINT,
        }

    class Parser(_QuotedFunctionNames, MySQL.Parser):
        SIGNED_TO_UNSIGNED_TYPE_TOKEN = _UNSIGNED_TYPES

        def _parse_types(self, *args: Any, **kwargs: Any) -> exp.Expression | None:
            start = self._index
            this = super()._parse_types(*args, **kwargs)
            if not isinstance(this, exp.DataType):
                return this
            tokens = self._tokens[start : self._index]
            if this.is_type(exp.DataType.Type.ARRAY) and any(
                token.token_type == TokenType.L_BRACKET for token in tokens
            ):
                self.raise_error("MySQL has no bracketed array types")
            if this.is_type(exp.DataType.Type.UDECIMAL) and tokens[0].text.upper() in {
                "NUMERIC",
                "FIXED",
            }:
                self.raise_error("UNSIGNED applies to DECIMAL, not NUMERIC or FIXED")
            if this.is_type(*exp.DataType.NUMERIC_TYPES):
                self._match_text_seq("SIGNED")  # type: ignore[no-untyped-call]
            return this


class _SQLite(SQLite):
    """SQLite with ``INT8`` as ``BIGINT`` and MySQL-style ``UNSIGNED``, as sqlparser-rs reads it."""

    class Tokenizer(SQLite.Tokenizer):
        KEYWORDS = {**SQLite.Tokenizer.KEYWORDS, "INT8": TokenType.BIGINT}

    class Parser(_QuotedFunctionNames, SQLite.Parser):
        SIGNED_TO_UNSIGNED_TYPE_TOKEN = _UNSIGNED_TYPES


class _Postgres(Postgres):
    """PostgreSQL with array casts read as sqlparser-rs reads them.

    ``INT[3]`` comes from ``SUPPORTS_FIXED_SIZE_ARRAYS``; ``INT ARRAY`` and
    ``INT ARRAY[3]`` anywhere in a list come from ``_parse_types``.
    """

    SUPPORTS_FIXED_SIZE_ARRAYS = True

    class Parser(_QuotedFunctionNames, Postgres.Parser):
        def _parse_types(self, *args: Any, **kwargs: Any) -> exp.Expression | None:
            # schema=True would make sqlglot read `INT ARRAY, b INT` as INT[b INT].
            this = super()._parse_types(*args, **{**kwargs, "schema": False})
            start = self._index
            if not isinstance(this, exp.DataType) or not self._match(TokenType.ARRAY):  # type: ignore[no-untyped-call]
                return this
            if not self._match(TokenType.L_BRACKET):  # type: ignore[no-untyped-call]
                return exp.DataType(this=exp.DataType.Type.ARRAY, expressions=[this], nested=True)
            size = self._parse_number()
            if size is None or not self._match(TokenType.R_BRACKET):  # type: ignore[no-untyped-call]
                self._retreat(start)
                return this
            return exp.DataType(
                this=exp.DataType.Type.ARRAY, expressions=[this], values=[size], nested=True
            )


_DIALECTS: dict[str, type[Dialect]] = {
    "mysql": _MySQL,
    "sqlite": _SQLite,
    "postgres": _Postgres,
    "postgresql": _Postgres,
}


def _parse(sql: str, dialect: str) -> list[exp.Expression | None]:
    """Parse like sqlglot, but read types the way sqlparser-rs (the Rust backend) does.

    In PostgreSQL, MySQL and SQLite, an ``ARRAY`` type needs an element type
    (``INT[]``, ``INT ARRAY``) and a fixed size must be an integer literal
    (``INT[3]``, not ``INT[n]``). Other dialects go to sqlglot unchanged.

    Raises:
        ParseError: ``sql`` is invalid or uses an unsupported type.
    """
    known = _DIALECTS.get(dialect.lower())
    statements = parse(sql, dialect=known or dialect)
    if known is None:
        return statements
    for statement in statements:
        for dt in statement.find_all(exp.DataType) if statement else ():
            if dt.this != exp.DataType.Type.ARRAY:
                continue
            if not dt.expressions:
                raise ParseError("ARRAY type needs an element type, e.g. INT[]")
            if not all(
                isinstance(size, exp.Literal) and size.is_int
                for size in dt.args.get("values") or []
            ):
                raise ParseError("ARRAY size must be an integer literal, e.g. INT[3]")
    return statements


# psycopg placeholders (``%s``, ``%b``, ``%t``, ``%(name)s``, ...) and the ``%%`` escape.
_PLACEHOLDER = re.compile(r"%%|%(?:\([^)]+\))?[sbt]")


def _query_input(sql: str) -> str:
    """``sql`` with placeholders as ``NULL``, a value of unknown type, and ``%%`` as ``%``.

    Like psycopg, literals and comments are not skipped; any query this mangles
    fails to parse and stays uninferred rather than mistyped.

    Example:
        >>> _query_input("SELECT %s, %(a-b)t, 5 %% 2")
        'SELECT NULL, NULL, 5 % 2'
    """
    return _PLACEHOLDER.sub(lambda m: "%" if m[0] == "%%" else "NULL", sql)


def _parse_one(sql: str, dialect: str) -> exp.Expression:
    """The first statement of ``sql``, like sqlglot's ``parse_one``, with ``_parse``'s checks."""
    statements = _parse(sql, dialect)
    if not statements or statements[0] is None:
        raise ParseError(f"No expression was parsed from '{sql}'")
    return statements[0]


def _leftmost_select(node: exp.Expression | None) -> exp.Select | None:
    """The SELECT whose columns a query returns: the left arm of a UNION, unparenthesized."""
    while not isinstance(node, exp.Select):
        if isinstance(node, exp.SetOperation):
            node = node.left
        elif isinstance(node, exp.Subquery):
            node = node.this
        else:
            return None
    return node


def _scope(relations: list[exp.Expression]) -> list[tuple[str, str]]:
    """``(reference, table)`` pairs for plain tables, in query order."""
    return [
        ((table.alias or table.name).lower(), table.name.lower())
        for table in relations
        if isinstance(table, exp.Table)
    ]


def _relations(select: exp.Select) -> list[exp.Expression]:
    """Sources of a SELECT: its FROM relation, then joined ones (``FROM a, b`` is a join)."""
    from_ = select.args.get("from") or select.args.get("from_")  # sqlglot 30 renamed the key
    relations = [from_.this] if from_ else []
    return relations + [join.this for join in select.args.get("joins") or []]


def _select_scope(select: exp.Select) -> list[tuple[str, str]]:
    """Scope of a SELECT: its FROM table, then joined tables."""
    return _scope(_relations(select))


def _star_resolvable(select: exp.Select, table_ref: str | None) -> bool:
    """Whether ``*`` (or ``t.*``) reads only plain tables, so the schema gives its columns.

    Subqueries and CTEs have no schema entry, and ``USING``/``NATURAL`` joins merge
    columns, so their stars are unresolved. Mirrors ``star_resolvable`` in Rust.
    """
    relations = _relations(select)
    if table_ref:
        relations = [r for r in relations if r.alias_or_name.lower() == table_ref.lower()]
    elif any(
        join.args.get("using") or join.args.get("method") == "NATURAL"
        for join in select.args.get("joins") or []
    ):
        return False
    ctes = {cte.alias_or_name.lower() for cte in select.root().find_all(exp.CTE)}
    return bool(relations) and all(
        isinstance(r, exp.Table) and r.name.lower() not in ctes for r in relations
    )


def _resolve(reference: str, scope: list[tuple[str, str]]) -> str:
    """The table an alias (or table name) refers to."""
    reference = reference.lower()
    return next((table for ref, table in scope if ref == reference), reference)


def _keyword_at(sql_upper: str, keyword: str, i: int) -> bool:
    end = i + len(keyword)
    return (
        sql_upper.startswith(keyword, i)
        and (i == 0 or not _is_word(sql_upper[i - 1]))
        and (end == len(sql_upper) or not _is_word(sql_upper[end]))
    )


def _is_word(char: str) -> bool:
    return char.isalnum() or char == "_"


def _top_level(sql_upper: str, keyword: str, start: int = 0) -> int:
    """Index of ``keyword`` outside parentheses at or after ``start``, or -1."""
    depth = 0
    for i in range(len(sql_upper)):
        char = sql_upper[i]
        depth += (char == "(") - (char == ")")
        if i >= start and depth == 0 and _keyword_at(sql_upper, keyword, i):
            return i
    return -1


def _split_top_level(text: str) -> list[str]:
    """Split on commas outside parentheses."""
    items, depth, start = [], 0, 0
    for i, char in enumerate(text):
        depth += (char == "(") - (char == ")")
        if char == "," and depth == 0:
            items.append(text[start:i].strip())
            start = i + 1
    return [*items, text[start:].strip()]

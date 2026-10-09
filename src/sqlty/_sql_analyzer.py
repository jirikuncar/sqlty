"""
SQL analysis utilities using sqlglot to extract column and type information.
"""

from pathlib import Path
from typing import NamedTuple

from sqlglot import exp, parse, parse_one

from sqlty._type_codec import TupleCodec, TypeCodec

# Module-level default to avoid re-creating per call
_default_codec = TupleCodec()


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
                ast = parse_one(statement, dialect=self.dialect)

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
        table_name_lower = table_name.lower()
        column_name_lower = column_name.lower()

        if table_name_lower in self.schema:
            return self.schema[table_name_lower].get(column_name_lower)

        return None

    def _process_projections(
        self,
        projections: list[exp.Expression],
        ast: exp.Expression,
        select_expr: exp.Select | None = None,
    ) -> list[ColumnInfo]:
        """
        Process a list of projection expressions and extract column information.

        Args:
            projections: List of projection expressions to process
            ast: The full AST for schema lookups
            select_expr: Optional SELECT expression for FROM clause lookups

        Returns:
            List of ColumnInfo objects with extracted type information
        """
        columns: list[ColumnInfo] = []

        for projection in projections:
            col_name = projection.alias_or_name
            python_type: str | None = None
            sql_type: str | None = None
            table_name: str | None = None

            # Try to extract type information using match statement
            match projection:
                case exp.Alias(this=exp.Cast(to=cast_to)):
                    # Aliased cast: SELECT/RETURNING (expr)::TYPE AS alias
                    sql_type = str(cast_to)
                    python_type = self._sql_to_python_type(sql_type)
                case exp.Cast(to=cast_to):
                    # Explicit cast: SELECT/RETURNING id::INTEGER
                    sql_type = str(cast_to)
                    python_type = self._sql_to_python_type(sql_type)
                case exp.Column(table=table_name) if self.schema and table_name:
                    # Column with explicit table name and schema available
                    sql_type = self._get_column_type_from_schema(table_name, col_name)
                    if sql_type:
                        python_type = self._sql_to_python_type(sql_type)
                case exp.Column() if self.schema:
                    # Column without table name - try to find table from AST
                    # For RETURNING, find the first table in the statement
                    table_expr = ast.find(exp.Table)
                    if table_expr:
                        table_name = table_expr.name
                        sql_type = self._get_column_type_from_schema(table_name, col_name)
                        if sql_type:
                            python_type = self._sql_to_python_type(sql_type)
                    # For SELECT, check FROM clause
                    elif select_expr:
                        from_expr = select_expr.find(exp.From)
                        if from_expr:
                            for table in from_expr.find_all(exp.Table):
                                table_name = table.name
                                sql_type = self._get_column_type_from_schema(table_name, col_name)
                                if sql_type:
                                    python_type = self._sql_to_python_type(sql_type)
                                    break
                case exp.Count():
                    # COUNT(*) always returns integer
                    python_type = "int"
                    sql_type = "INTEGER"
                case exp.Sum(this=exp.Cast(to=cast_to)):
                    # SUM has an explicit cast: SUM(column::TYPE)
                    sql_type = str(cast_to)
                    python_type = self._sql_to_python_type(sql_type)
                case exp.Sum(this=exp.Column(name=column_name, table=table_name)) if (
                    self.schema and table_name
                ):
                    # SUM of column with explicit table name and schema available
                    # First check if table exists in schema
                    if table_name.lower() in self.schema:
                        sql_type = self._get_column_type_from_schema(table_name, column_name)
                        if sql_type:
                            python_type = self._sql_to_python_type(sql_type)
                        else:
                            # Table in schema but column not found
                            raise RuntimeError(
                                f"Column '{column_name}' not found in table '{table_name}' in schema. "
                                f"If you want to query columns not in the schema, use an explicit cast: "
                                f"SUM({column_name}::TYPE)"
                            )
                    else:
                        # Table not in schema, fall back to float
                        python_type = "float"
                        sql_type = "NUMERIC"
                case exp.Sum(this=exp.Column(name=column_name)) if self.schema:
                    # SUM of column without table name - try to find table from query
                    table_name = None
                    table_expr = ast.find(exp.Table)
                    if table_expr:
                        table_name = table_expr.name
                    elif select_expr:
                        from_expr = select_expr.find(exp.From)
                        if from_expr:
                            for table in from_expr.find_all(exp.Table):
                                table_name = table.name
                                break

                    if table_name is not None:
                        # Check if table exists in schema
                        if table_name.lower() in self.schema:
                            sql_type = self._get_column_type_from_schema(table_name, column_name)
                            if sql_type:
                                python_type = self._sql_to_python_type(sql_type)
                            else:
                                # Table in schema but column not found
                                raise RuntimeError(
                                    f"Column '{column_name}' not found in table '{table_name}' in schema. "
                                    f"If you want to query columns not in the schema, use an explicit cast: "
                                    f"SUM({column_name}::TYPE)"
                                )
                        # else: table not in schema, fall through to default

                    if not python_type:
                        # Table not found in query or not in schema, fall back to float
                        python_type = "float"
                        sql_type = "NUMERIC"
                case exp.Sum():
                    # Default to float for SUM without explicit cast or schema
                    python_type = "float"
                    sql_type = "NUMERIC"
                case exp.Func() as func:
                    # Other functions - try to infer from function name
                    python_type = self._infer_from_function(func)

            columns.append(ColumnInfo(col_name, python_type, sql_type))

        return columns

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
            statements = parse(sql, dialect=self.dialect)
        except Exception:
            return True
        statement = statements[0] if statements else None
        if isinstance(statement, (exp.Insert, exp.Update, exp.Delete, exp.Merge)):
            return bool(statement.args.get("returning"))
        return isinstance(statement, (exp.Query, exp.Values))

    def analyze_select(self, sql: str) -> list[ColumnInfo] | None:
        """
        Analyze a SQL query and extract column information from result set.

        Supports:
        - SELECT queries
        - UPDATE/DELETE/INSERT/MERGE with RETURNING clause

        Args:
            sql: SQL query string to analyze

        Returns:
            List of ColumnInfo objects if the query returns results, None otherwise.
            Returns None for DML statements (UPDATE/DELETE/INSERT/MERGE) without RETURNING.
        """
        try:
            ast = parse_one(sql, dialect=self.dialect)
        except Exception:
            return None

        # Check for RETURNING clause in UPDATE/DELETE/INSERT/MERGE statements
        returning_expr = ast.find(exp.Returning)
        if returning_expr:
            return self._process_projections(returning_expr.expressions, ast)

        # Find the SELECT expression
        select_expr = ast.find(exp.Select)
        if not select_expr:
            return None

        return self._process_projections(select_expr.expressions, ast, select_expr)

    def _sql_to_python_type(self, sql_type: str) -> str:
        """Convert SQL type to Python type."""
        # Normalize the type string
        normalized = sql_type.upper().split("(")[0].strip()
        return self.SQL_TO_PYTHON_TYPE.get(normalized, "str")

    def _infer_from_function(self, func: exp.Func) -> str | None:
        """Infer Python type from SQL function."""
        func_name = type(func).__name__.upper()

        match func_name:
            case "COUNT" | "LENGTH" | "POSITION":
                return "int"
            case "AVG" | "SUM" | "STDDEV":
                return "float"
            case (
                "UPPER"
                | "LOWER"
                | "TRIM"
                | "CONCAT"
                | "CURRENTTIMESTAMP"
                | "CURRENTDATE"
                | "NOW"
                | "CURRENT_DATE"
                | "CURRENT_TIMESTAMP"
            ):
                return "str"
            case _:
                return None

    def expand_star(self, sql: str) -> str | None:
        """Expand SELECT * into explicit column list using loaded schema.

        Replaces `SELECT *` or `SELECT t.*` with `SELECT col1::TYPE, col2::TYPE, ...`
        using column definitions from the loaded schema.

        Args:
            sql: SQL query string containing SELECT *

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
            ast = parse_one(sql, dialect=self.dialect)
        except Exception:
            return None

        select_expr = ast.find(exp.Select)
        if not select_expr:
            return None

        # Check if any projection is a Star or Column(name='*')
        has_star = False
        for proj in select_expr.expressions:
            if isinstance(proj, exp.Star):
                has_star = True
                break
            if isinstance(proj, exp.Column) and isinstance(proj.this, exp.Star):
                has_star = True
                break

        if not has_star:
            return None

        # Determine target table from FROM clause
        from_expr = select_expr.find(exp.From)
        if not from_expr:
            return None

        # Build alias-to-table mapping
        alias_map: dict[str, str] = {}
        tables_in_from: list[str] = []
        for table_expr in from_expr.find_all(exp.Table):
            table_name = table_expr.name.lower()
            tables_in_from.append(table_name)
            if table_expr.alias:
                alias_map[table_expr.alias.lower()] = table_name
            else:
                alias_map[table_name] = table_name

        # Also check JOINs
        for join_expr in select_expr.find_all(exp.Join):
            for table_expr in join_expr.find_all(exp.Table):
                table_name = table_expr.name.lower()
                tables_in_from.append(table_name)
                if table_expr.alias:
                    alias_map[table_expr.alias.lower()] = table_name
                else:
                    alias_map[table_name] = table_name

        # Build new projections
        new_projections: list[str] = []
        for proj in select_expr.expressions:
            if isinstance(proj, exp.Star):
                # Unqualified star — expand all tables in FROM order
                expanded = self._expand_unqualified_star(tables_in_from)
                if expanded is None:
                    return None
                new_projections.extend(expanded)
            elif isinstance(proj, exp.Column) and isinstance(proj.this, exp.Star):
                # Qualified star (t.*) — expand specific table
                table_ref = proj.table.lower() if proj.table else None
                if table_ref and table_ref in alias_map:
                    real_table = alias_map[table_ref]
                    expanded = self._expand_table_columns(real_table)
                    if expanded is None:
                        return None
                    new_projections.extend(expanded)
                else:
                    return None
            else:
                # Non-star projection — keep as-is
                new_projections.append(proj.sql(dialect=self.dialect))

        # Reconstruct the SQL with expanded columns
        columns_sql = ", ".join(new_projections)

        # Replace the SELECT ... FROM with expanded columns
        # Use string reconstruction for reliability
        sql_upper = sql.upper()
        select_idx = sql_upper.find("SELECT")
        from_idx = sql_upper.find("FROM")
        if select_idx == -1 or from_idx == -1:
            return None

        prefix = sql[:select_idx] + "SELECT "
        suffix = sql[from_idx:]
        return prefix + columns_sql + " " + suffix

    def _expand_unqualified_star(self, tables: list[str]) -> list[str] | None:
        """Expand unqualified * for all tables in FROM clause."""
        all_columns: list[str] = []
        for table_name in tables:
            cols = self._expand_table_columns(table_name)
            if cols is None:
                return None
            all_columns.extend(cols)
        return all_columns

    def _expand_table_columns(self, table_name: str) -> list[str] | None:
        """Expand a single table into col::TYPE pairs."""
        table_lower = table_name.lower()
        if table_lower not in self.schema:
            return None

        columns = []
        for col_name, sql_type in self.schema[table_lower].items():
            columns.append(f"{col_name}::{sql_type}")
        return columns

    def generate_return_type(
        self,
        sql: str,
        codec: TypeCodec | None = None,
    ) -> str | None:
        """
        Generate a Python return type annotation for a SQL query.

        Args:
            sql: SQL query string to analyze
            codec: Optional TypeCodec for formatting the result.
                   Defaults to TupleCodec() which produces "tuple[int, str]".

        For example:
        - "SELECT id, name FROM users" -> "tuple[int, str]" (if types can be inferred)
        - "SELECT id, name FROM users" -> "tuple[str, str]" (if types can't be inferred)
        """
        if codec is None:
            codec = _default_codec

        columns = self.analyze_select(sql)
        if not columns:
            return None

        # Build type list
        types: list[str] = []
        for col in columns:
            if col.python_type:
                types.append(col.python_type)
            else:
                # Default to str if type can't be inferred
                types.append("str")

        return codec.format_type(types)

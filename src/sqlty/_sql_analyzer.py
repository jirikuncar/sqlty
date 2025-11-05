"""
SQL analysis utilities using sqlglot to extract column and type information.
"""

from pathlib import Path
from typing import NamedTuple

from sqlglot import exp, parse_one


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

            # Try to extract type information using match statement
            match projection:
                case exp.Alias(this=exp.Cast() as cast_expr):
                    # Aliased cast: SELECT/RETURNING (expr)::TYPE AS alias
                    sql_type = str(cast_expr.to)
                    python_type = self._sql_to_python_type(sql_type)
                case exp.Cast():
                    # Explicit cast: SELECT/RETURNING id::INTEGER
                    sql_type = str(projection.to)
                    python_type = self._sql_to_python_type(sql_type)
                case exp.Column():
                    # Simple column: SELECT/RETURNING id
                    # Try to infer type from schema if available
                    if self.schema:
                        # First, try to get table from the column itself
                        table_name = projection.table
                        if table_name:
                            sql_type = self._get_column_type_from_schema(table_name, col_name)
                            if sql_type:
                                python_type = self._sql_to_python_type(sql_type)
                        else:
                            # If no table specified on column, try to find it in the AST
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
                                        sql_type = self._get_column_type_from_schema(
                                            table_name, col_name
                                        )
                                        if sql_type:
                                            python_type = self._sql_to_python_type(sql_type)
                                            break
                case exp.Count():
                    # COUNT(*) always returns integer
                    python_type = "int"
                    sql_type = "INTEGER"
                case exp.Sum():
                    # SUM - try to infer type from what's being summed
                    if hasattr(projection, 'this') and isinstance(projection.this, exp.Cast):
                        # SUM has an explicit cast: SUM(column::TYPE)
                        sql_type = str(projection.this.to)
                        python_type = self._sql_to_python_type(sql_type)
                    else:
                        # Default to float for SUM without explicit cast
                        python_type = "float"
                        sql_type = "NUMERIC"
                case exp.Func():
                    # Other functions - try to infer from function name
                    python_type = self._infer_from_function(projection)

            columns.append(ColumnInfo(col_name, python_type, sql_type))

        return columns

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

    def generate_return_type(self, sql: str) -> str | None:
        """
        Generate a Python return type annotation for a SQL query.

        For example:
        - "SELECT id, name FROM users" -> "tuple[int, str]" (if types can be inferred)
        - "SELECT id, name FROM users" -> "tuple[str, str]" (if types can't be inferred)
        """
        columns = self.analyze_select(sql)
        if not columns:
            return None

        # Build tuple type
        types: list[str] = []
        for col in columns:
            if col.python_type:
                types.append(col.python_type)
            else:
                # Default to str if type can't be inferred
                types.append("str")

        if len(types) == 1:
            return f"tuple[{types[0]}]"
        else:
            return f"tuple[{', '.join(types)}]"

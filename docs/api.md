# API Reference

Complete API documentation for SQLType.

## Core Classes

### `@sqlty` Decorator

Decorator to mark functions for SQLTy type inference and stub generation.

```python
from sqlty import sqlty

@sqlty
def sql(query: str) -> Any:
    """Execute a SQL query and return typed results."""
    return query
```

**Type Inference:**

The return type is inferred based on:

1. **Explicit type casts in SQL:**
   ```python
   sql("SELECT id::INTEGER, name::TEXT FROM users")
   # Returns: SQL[tuple[int, str]]
   ```

2. **Column analysis:**
   ```python
   sql("SELECT COUNT(*)::INTEGER FROM users")
   # Returns: SQL[tuple[int]]
   ```

3. **Fallback to Any:**
   ```python
   sql("SELECT * FROM users")
   # Returns: SQL[tuple[Any, ...]]
   ```

**Stub Generation:**

The `sqlty` tool creates `@overload` declarations for each unique query:

```python
# Generated stub
@overload
def sql(
    cls,
    query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
) -> SQL[tuple[int, str]]: ...
```

---

### SQL[T]

Generic type representing SQL query results.

```python
from sqlty import SQL
```

**Type Parameters:**
- `T`: Represents the row structure

**Protocol:**
```python
class SQL[T](str):
    pass
```

---

## Configuration

### pyproject.toml Configuration

SQLTy can be configured via `[tool.sqlty]` in your `pyproject.toml` file.

#### Basic Settings

```toml
[tool.sqlty]
dialect = "postgresql"
format-command = "ruff format --stdin-filename {filename} -"
```

**Available settings:**
- `dialect`: SQL dialect to use (default: `"postgres"`)
- `format-command`: External formatter command with `{filename}` placeholder
- `schemas`: Dictionary mapping registry names to schema file paths

#### Schema Mappings

```toml
[tool.sqlty]
schemas = { users_queries = "users.sql", orders_queries = "orders.sql" }
```

#### Function Configuration

Configure functions for analysis without using the `@sqlty` decorator:

```toml
[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"
```

**Required fields:**
- `import_path` (string): Full import path to the function (e.g., `"module.submodule.function_name"`)
- `query_param` (string): Name of the parameter containing the SQL query string

**Optional fields:**
- `return_type_param` (string): Name of the parameter specifying the expected return type
- `schema` (string): Path to SQL schema file for type inference
- `return_type_template` (string): Template for return type with `{sqlty}` placeholder (e.g., `"list[{sqlty}]"`)
- `self_type_template` (string): Reserved for future method support (not currently used)
- `imports` (list of strings): Import statements needed for the stub file

**Example with all fields:**
```toml
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
schema = "schema.sql"
return_type_template = "Optional[{sqlty}]"
imports = [
    "from typing import Optional",
    "from typing import Any"
]
```

**Multiple function configurations:**
```toml
[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"

[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
schema = "schema.sql"
```

**Limitations:**
- Class methods (e.g., `"module.Class.method"`) are not currently supported
- Use module-level functions instead

See the [Advanced Usage](advanced-usage.md#function-configuration) guide and [`examples/function_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/function_config) for complete examples.

---

## Generator CLI

### Command Line Interface

```bash
sqlty <function_name> <source> [options]
```

Generate `.pyi` stub files from SQL queries in source code.

#### Positional Arguments

**`function_name`**

Name of the function to generate overloads for (typically `sql`).

**`source`**

Path to source file or directory to analyze:
- **File**: Analyze a single Python file
- **Directory**: Recursively analyze all `.py` files (except those starting with `_`)

#### Options

**`-o, --output PATH`**

Output `.pyi` file path.

- **Auto-detection** (default): Determines output based on registry imports
- **Explicit path**: Uses specified path

```bash
# Auto-detect output path
sqlty app.py

# Explicit output
sqlty app.py --output stubs/app.pyi
```

**`--dialect DIALECT`**

SQL dialect for query analysis (default: `postgres`).

Supported dialects:
- `postgres` (PostgreSQL) - default
- `mysql` (MySQL/MariaDB)
- `sqlite` (SQLite)
- `bigquery` (Google BigQuery)
- `snowflake` (Snowflake)
- `redshift` (Amazon Redshift)
- `oracle` (Oracle Database)
- `mssql` (Microsoft SQL Server)
- `clickhouse` (ClickHouse)
- `presto` (Presto/Trino)

```bash
sqlty . --dialect mysql
```

**`--mode=incremental`**

Add new overloads to existing stub file without removing old ones.

```bash
# Initial generation
sqlty app.py

# Add new queries (preserves existing overloads)
sqlty app.py --mode=incremental
```

Use cases:
- Iterative development
- Large codebases where full regeneration is slow
- Preserving overloads for temporarily disabled code

**`--mode=prune`**

Remove overloads for queries that no longer exist in source code.

```bash
sqlty . --mode=prune
```

Use it to avoid accumulation of unused overloads after removing queries.

**`--schema PATH | --schema registry:PATH`**

Provide database schema files to improve type inference (e.g., when queries don’t use explicit `::TYPE` casts). You can:

- Pass a single global schema path (applies to all registries):

```bash
sqlty . --schema schema.sql
```

- Pass one or more registry-specific mappings using `registry:PATH` (repeat `--schema` to add multiple mappings):

```bash
sqlty . \
    --schema users_queries:users.sql \
    --schema orders_queries:orders.sql
```

Precedence when selecting the schema for a registry:

1. Registry-specific mapping from `--schema registry:PATH`
2. Global mapping from `--schema PATH`
3. `__schema_path__` attribute declared on the registry function

Examples of a registry with `__schema_path__`:

```python
from sqlty import sqlty

@sqlty
def users_sql[T](query: T) -> T:
    __schema_path__ = "schema.sql"
    return query
```

CLI mappings always override `__schema_path__` when provided.

#### Exit Codes

- `0`: Success
- `1`: Error (source path doesn't exist, parse error, etc.)


---

## Type Mappings

### SQL to Python Type Mapping

The generator maps SQL type casts to Python types:

| SQL Type | Python Type | Import |
|----------|-------------|--------|
| `INTEGER`, `BIGINT`, `SMALLINT`, `INT` | `int` | Built-in |
| `TEXT`, `VARCHAR`, `CHAR`, `STRING` | `str` | Built-in |
| `BOOLEAN`, `BOOL` | `bool` | Built-in |
| `FLOAT`, `DOUBLE`, `REAL` | `float` | Built-in |
| `DECIMAL`, `NUMERIC` | `Decimal` | `from decimal import Decimal` |
| `DATE` | `datetime.date` | `import datetime` |
| `TIMESTAMP`, `TIMESTAMPTZ`, `DATETIME` | `datetime.datetime` | `import datetime` |
| `TIME`, `TIMETZ` | `datetime.time` | `import datetime` |
| `INTERVAL` | `datetime.timedelta` | `import datetime` |
| `BYTEA`, `BLOB`, `BINARY` | `bytes` | Built-in |
| `JSON`, `JSONB` | `Any` | `from typing import Any` |
| `UUID` | `uuid.UUID` | `import uuid` |

### Using Type Casts

**Basic types:**
```sql
SELECT
    id::INTEGER,           -- int
    name::TEXT,            -- str
    is_active::BOOLEAN,    -- bool
    price::FLOAT          -- float
FROM products
```

**Date/time types:**
```sql
SELECT
    created_date::DATE,              -- datetime.date
    created_at::TIMESTAMP,           -- datetime.datetime
    scheduled_time::TIME,            -- datetime.time
    duration::INTERVAL               -- datetime.timedelta
FROM events
```

**Numeric types:**
```sql
SELECT
    count::BIGINT,          -- int
    amount::DECIMAL,        -- Decimal
    ratio::DOUBLE           -- float
FROM statistics
```

**Binary data:**
```sql
SELECT
    document::BYTEA,        -- bytes
    file_content::BLOB      -- bytes
FROM files
```

---

## Generated Stub Structure

### Stub File Format

Generated `.pyi` files have this structure:

```python
from typing import overload, Literal
from sqltype import SQL

@overload
def sql(
    cls,
    query: Literal["""
SELECT id::INTEGER, name::TEXT FROM users
"""],
) -> SQL[tuple[int, str]]: ...

@overload
def sql(
    cls,
    query: Literal["SELECT COUNT(*)::INTEGER FROM users"],
) -> SQL[tuple[int]]: ...

@overload
def sql[T](cls, query: T) -> T: ...
```


### Fallback Overload

Every stub includes a generic fallback:

```python
@overload
def sql[T](cls, query: T) -> T: ...
```

This ensures dynamic queries still type-check:

```python
query_str = get_dynamic_query()
result = sql(query_str)  # Type: str (not SQL[...])
```

---

## Type Checking

### Using with mypy

**Basic configuration (pyproject.toml):**

```toml
[tool.mypy]
python_version = "3.14"
strict = true
warn_return_any = true
warn_unused_configs = true
```

**Run mypy:**

```bash
mypy .
```

**Type errors:**

```python
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]

# ✓ OK
for user_id, name in result:
    pass

# ✗ Error: too many values to unpack
for user_id, name, email in result:
    pass

# ✗ Error: incompatible types
first_row = next(result)
user_id: str = first_row[0]  # Expected int, got str
```

---

## Error Handling

### Parse Errors

If the generator encounters invalid Python:

```
Warning: Failed to analyze app.py: invalid syntax (app.py, line 15)
```

The generator continues with other files and creates stubs for valid queries.

### SQL Parse Errors

If sqlglot can't parse a query:

```
Warning: Failed to parse SQL in app.py:23
```

The overload is still generated but with `Any` types:

```python
@overload
def sql(cls, query: Literal["INVALID SQL"]) -> SQL[tuple[Any, ...]]: ...
```

---

## Performance Considerations

### Large Codebases

For large projects:

1. **Use `--mode=incremental` during development:**
   ```bash
   sqlty . --mode=incremental
   ```

2. **Use `--check` in CI/CD:**
   ```bash
   sqlty . --check
   ```

### Stub File Size

Each unique query adds one overload. To keep stubs manageable:

- Deduplicate queries across files
- Use parameterized queries instead of string interpolation
- Consider splitting into multiple registries

---

## Integration Examples

### With FastAPI

```python
from fastapi import FastAPI
from queries import sql
from sqlty.drivers.psycopg import execute

app = FastAPI()

@app.get("/users")
async def get_users() -> list[tuple[int, str]]:
    query = sql("SELECT id::INTEGER, name::TEXT FROM users")
    # Type: SQL[tuple[int, str]]

    result = execute(conn, query, {})
    # Type: Iterator[tuple[int, str]]

    return list(result)
```

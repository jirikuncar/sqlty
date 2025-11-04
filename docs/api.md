# API Reference

Complete API documentation for SQLType.

## Core Classes

### SQLRegistry

Base class for SQL query registries.

```python
from sqltype import SQLRegistry

class Query(SQLRegistry):
    """Your SQL query registry."""
    pass
```

#### Class Methods

##### `sql(query: str) -> SQL[T]`

Execute a SQL query and return typed results.

**Signature:**
```python
@classmethod
def sql(cls, query: str) -> SQL[T]: ...
```

**Parameters:**
- `query` (str): SQL query string. Can include:
  - Type casts (e.g., `::INTEGER`, `::TEXT`)
  - Named parameters (e.g., `:user_id`)
  - Positional parameters (e.g., `?`)

**Returns:**
- `SQL[T]`: Iterator over query results with inferred type `T`

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

**Example:**
```python
class Query(SQLRegistry):
    pass

sql = Query.sql

# Create typed query
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

# Execute query
result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]

for user_id, name in result:
    print(f"{user_id}: {name}")

# Parameterized query
user_query = sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT
    FROM users
    WHERE id = :user_id
""")
# Type: SQL[tuple[int, str, str]]

user_result = execute(conn, user_query, {"user_id": 123})
# Type: Iterator[tuple[int, str, str]]

first_user = next(user_result, None)
```

**Stub Generation:**

The `sqlty` tool creates `@overload` declarations for each unique query:

```python
# Generated stub
@overload
@classmethod
def sql(
    cls,
    query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
) -> SQL[tuple[int, str]]: ...

@overload
@classmethod
def sql[T](cls, query: T) -> T: ...  # Fallback for dynamic queries
```

---

### SQL[T]

Generic type representing SQL query results.

```python
from sqltype import SQL
```

**Type Parameters:**
- `T`: Tuple type representing the row structure

**Protocol:**
```python
class SQL(Protocol[T]):
    def __iter__(self) -> Iterator[T]: ...
```

**Usage:**

```python
from sqltype import SQL

def get_users() -> SQL[tuple[int, str]]:
    return sql("SELECT id::INTEGER, name::TEXT FROM users")

# Type-safe iteration
query = get_users()
# Type: SQL[tuple[int, str]]

result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]

for user_id, name in result:
    print(f"User {user_id}: {name}")

# Convert to list
users_list: list[tuple[int, str]] = list(result)

# Get first result
first_user: tuple[int, str] | None = next(result, None)
```

**Type Safety:**

```python
# ✓ Correct usage
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]

for user_id, name in result:  # OK
    pass

# ✗ Type error
for user_id, name, email in result:  # Error: too many values to unpack
    pass
```

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
sqlty app.py --output stubs/queries.pyi
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

**`--class-name NAME`**

Name of the generated Query class (default: `Query`).

```bash
sqlty . --class-name MyQuery
```

Output:
```python
class MyQuery(SQLRegistry):
    @overload
    @classmethod
    def sql(...) -> ...: ...
```

**`--base-class NAME`**

Name of the base class (default: `SQLRegistry`).

```bash
sqlty . --base-class CustomRegistry
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

**`--mode=full`**

Remove overloads for queries that no longer exist in source code.

```bash
sqlty . --mode=full
```

This is the default for directory mode. Use it to:
- Clean up stubs after removing queries
- Ensure stubs match current source code exactly
- Avoid accumulation of unused overloads

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
3. `__schema_path__` attribute declared on the registry class

Examples of a registry with `__schema_path__`:

```python
from sqltype import SQLRegistry

class Users(SQLRegistry):
        __schema_path__ = "schema.sql"
```

CLI mappings always override `__schema_path__` when provided.

#### Exit Codes

- `0`: Success
- `1`: Error (source path doesn't exist, parse error, etc.)

#### Output

The generator prints information about the generation process:

```
Detected SQLRegistry subclass in 'queries', writing to queries.pyi
Generated stub file (full): queries.pyi
Found 6 call(s) to sql
```

For multiple registries:

```
Warning: Multiple import sources found: {'user_queries', 'post_queries'}. Using: user_queries
Detected SQLRegistry subclass in 'user_queries', writing to user_queries.pyi
Generated stub file (full): user_queries.pyi
Found 3 call(s) to sql
Detected SQLRegistry subclass in 'post_queries', writing to post_queries.pyi
Generated stub file (full): post_queries.pyi
Found 3 call(s) to sql
```

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
from sqltype import SQL, SQLRegistry

class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    # from app.py:15
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["""
    SELECT id::INTEGER, name::TEXT FROM users
    """],
    ) -> SQL[tuple[int, str]]: ...

    # from app.py:23
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT COUNT(*)::INTEGER FROM users"],
    ) -> SQL[tuple[int]]: ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

# Type alias for the sql function
sql = Query.sql
```

### Source Location Comments

Each overload includes a comment with the source location:

```python
# from app.py:15
```

This shows:
- Relative file path from the stub location
- Line number where the query appears

Benefits:
- Easy traceability from stub to source
- Helps during debugging
- Useful for code review

### Fallback Overload

Every stub includes a generic fallback:

```python
@overload
@classmethod
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

### Using with Pylance/Pyright

**VS Code settings (.vscode/settings.json):**

```json
{
  "python.analysis.typeCheckingMode": "strict",
  "python.analysis.useLibraryCodeForTypes": true
}
```

**Type hints in editor:**

Hover over variables to see inferred types:
```python
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]
```

---

## Advanced Usage

### Custom Registry Classes

Create domain-specific registries:

```python
from sqlty.drivers.psycopg import execute

class UserQueries(SQLRegistry):
    """User-related queries."""

    @classmethod
    def get_by_email(cls, email: str):
        query = cls.sql("""
            SELECT id::INTEGER, name::TEXT
            FROM users
            WHERE email = :email
        """)
        # Type: SQL[tuple[int, str]]

        return execute(conn, query, {"email": email})
        # Type: Iterator[tuple[int, str]]

user_sql = UserQueries.sql
```

### Type Annotations in Registry

Add type hints for better IDE support:

```python
from sqltype import SQL, execute

class Query(SQLRegistry):
    @classmethod
    def get_users(cls) -> Iterator[tuple[int, str]]:
        query = cls.sql("SELECT id::INTEGER, name::TEXT FROM users")
        # Type: SQL[tuple[int, str]]

        return execute(conn, query, {})
        # Type: Iterator[tuple[int, str]]
```

### Dynamic Queries

For dynamic queries, use the fallback overload:

```python
def search_users(field: str, value: str):
    # Dynamic query - no type inference
    query = f"SELECT * FROM users WHERE {field} = :value"
    result = sql(query)  # Type: str (no SQL wrapper)

    # This won't have type safety
    return result
```

Consider using query builders or parameterized queries instead for safety.

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
@classmethod
def sql(cls, query: Literal["INVALID SQL"]) -> SQL[tuple[Any, ...]]: ...
```

### Missing Registry

If no registry is detected, a warning is shown:

```
Warning: No SQLRegistry subclass detected, using default Query class
```

A default stub is generated with the `--class-name` option.

---

## Performance Considerations

### Large Codebases

For large projects:

1. **Use `--mode=incremental` during development:**
   ```bash
   sqlty . --mode=incremental
   ```

2. **Use `--mode=full` in CI/CD:**
   ```bash
   sqlty . --mode=full
   ```

3. **Analyze specific directories:**
   ```bash
   sqlty src/queries/ --mode=full
   ```

### Stub File Size

Each unique query adds one overload. To keep stubs manageable:

- Deduplicate queries across files
- Use parameterized queries instead of string interpolation
- Consider splitting into multiple registries

---

## Integration Examples

### With pytest

```python
from queries import sql
from sqlty.drivers.psycopg import execute

def test_get_users():
    query = sql("SELECT id::INTEGER, name::TEXT FROM users")
    # Type: SQL[tuple[int, str]]

    result = execute(conn, query, {})
    # Type: Iterator[tuple[int, str]]

    users_list = list(result)

    # Type checker knows the shape
    user_id, name = users_list[0]
    assert isinstance(user_id, int)
    assert isinstance(name, str)
```

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

### With SQLAlchemy

```python
from sqlalchemy import create_engine
from queries import sql
from sqlty.drivers.psycopg import execute

engine = create_engine("postgresql://...")

with engine.connect() as conn:
    query = sql("SELECT id::INTEGER, name::TEXT FROM users")
    # Type: SQL[tuple[int, str]]

    result = execute(conn, query, {})
    # Type: Iterator[tuple[int, str]]

    # Process results with full type safety
    for user_id, name in result:
        print(f"{user_id}: {name}")
```

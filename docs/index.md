# SQLTy Documentation

Type-safe SQL queries for Python with automatic stub generation and runtime validation.

## Table of Contents

- [Motivation](motivation.md)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Basic Usage](#basic-usage)
- [Advanced Usage](#advanced-usage)
  - [Function Configuration](#function-configuration)
  - [Multi-Registry Pattern](#multi-registry-pattern)
  - [Custom SQL Dialects](#custom-sql-dialects)
  - [Incremental Mode](#incremental-mode)
  - [Schemas and Precedence](#schemas-and-precedence)
- [Internals](#internals)
  - [Architecture Overview](#architecture-overview)
  - [Type Inference](#type-inference)
  - [AST Analysis](#ast-analysis)
- [API Reference](#api-reference)
  - [@sqlty Decorator](#sqlty-decorator)
  - [SQL Type](#sql-type)
  - [Generator CLI](#generator-cli)
- [Examples](#examples)
- [Contributing](#contributing)

## Installation

### Using pip

```bash
pip install sqlty
```

### Using uv (recommended for development)

```bash
uv pip install sqlty
```

### From source

```bash
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty
uv pip install -e .
```

### Requirements

- Python 3.12+
- mypy (for type checking)
- sqlglot (for SQL parsing and type inference)

## Quick Start

1. **Define your SQL query function:**

```python
# queries.py
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    """Your SQL query function."""
    return query
```

2. **Write SQL queries in your code:**

```python
# app.py
from queries import sql
from sqlty.drivers.psycopg import execute

# Type-safe SQL query
query = sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT
    FROM users
    WHERE id = :id
""")
# Type: SQL[tuple[int, str, str]]

# Execute query with type safety
result = execute(conn, query, {"id": 123})
# Type: Iterator[tuple[int, str, str]]

# mypy knows the exact return type
for user_id, name, email in result:
    print(f"{name} <{email}>")
```

3. **Generate type stubs:**

```bash
sqlty . --mode=full
```

4. **Type check with mypy:**

```bash
mypy .
# Success: no issues found
```

## Basic Usage

### Creating a SQL Query Function

Use the `@sqlty` decorator to mark functions for type-safe SQL query execution:

```python
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    """Application-specific SQL query function with type-safe overloads."""
    return query
```

### Writing SQL Queries

Use the decorated `sql()` function to create typed queries, then execute them with the `execute()` function:

```python
from queries import sql
from sqlty.drivers.psycopg import execute

# Simple SELECT query
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]

# Query with WHERE clause
user_query = sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT
    FROM users
    WHERE id = :id
""")
# Type: SQL[tuple[int, str, str]]

user_result = execute(conn, user_query, {"id": 123})
# Type: Iterator[tuple[int, str, str]]

# Aggregation query
count_query = sql("SELECT COUNT(*)::INTEGER FROM users")
# Type: SQL[tuple[int]]

count_result = execute(conn, count_query, {})
# Type: Iterator[tuple[int]]
```

### Type Annotations in SQL

SQLTy uses PostgreSQL-style type casts to infer Python types:

| SQL Type Cast | Python Type |
|--------------|-------------|
| `::INTEGER` | `int` |
| `::TEXT` | `str` |
| `::BOOLEAN` | `bool` |
| `::FLOAT` | `float` |
| `::DOUBLE` | `float` |
| `::DECIMAL` | `Decimal` |
| `::DATE` | `datetime.date` |
| `::TIMESTAMP` | `datetime.datetime` |
| `::TIMESTAMPTZ` | `datetime.datetime` |
| `::INTERVAL` | `datetime.timedelta` |
| `::BYTEA` | `bytes` |

Example:

```python
result = sql("""
    SELECT
        id::INTEGER,
        name::TEXT,
        is_active::BOOLEAN,
        created_at::TIMESTAMP,
        birth_date::DATE,
        duration::INTERVAL
    FROM users
""")
# Type: SQL[tuple[int, str, bool, datetime.datetime, datetime.date, datetime.timedelta]]

# Execute with type safety
rows = execute(conn, result, {})
# Type: Iterator[tuple[int, str, bool, datetime.datetime, datetime.date, datetime.timedelta]]
```

### Generating Stub Files

The `sqlty` command analyzes your code and generates `.pyi` stub files:

```bash
# Generate stubs for a single file
sqlty queries.py

# Generate stubs for a directory (full mode with cleanup)
sqlty . --mode=full

# Incremental mode (add new overloads without removing old ones)
sqlty . --mode=incremental
```

The generator will:
1. Scan your Python files for `sql()` calls
2. Analyze the SQL queries using sqlglot
3. Infer return types from SQL type casts
4. Generate `.pyi` stub files with type overloads
5. Add source location comments for traceability

### Using the Generated Stubs

Once generated, the stub files are automatically picked up by mypy:

```python
from queries import sql
from sqlty.drivers.psycopg import execute

# mypy knows the exact return type
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

reveal_type(query)  # Revealed type is "SQL[tuple[int, str]]"

result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]

# Type errors are caught
for user_id, name, email in result:  # Error: too many values to unpack
    pass
```

## Advanced Usage

### Function Configuration

You can configure SQLTy to analyze functions without using the `@sqlty` decorator. This is useful for existing codebases, third-party libraries, or when you can't modify source code.

**Configuration in `pyproject.toml`:**

```toml
[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"
```

**Your existing code (no decorators needed):**

```python
# db.py
def execute_query(conn, query: str, **params):
    """Execute a query and return results."""
    # Implementation details...
    return []
```

**Usage with full type safety:**

```python
from db import execute_query

# Type inferred from SQL and schema
results = execute_query(
    conn,
    query="SELECT id::INTEGER, name::TEXT FROM users"
)
# Type: list[tuple[int, str]]
```

**Configuration options:**
- `import_path` (required): Full import path to function (e.g., `"module.function"`)
- `query_param` (required): Parameter name containing the SQL query
- `return_type_param` (optional): Parameter name specifying return type
- `schema` (optional): Path to SQL schema file
- `return_type_template` (optional): Template with `{sqlty}` placeholder
- `imports` (optional): List of import statements for the stub

See [Advanced Usage - Function Configuration](advanced-usage.md#function-configuration) and the [`examples/function_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/function_config) example for details.

### Multi-Registry Pattern

For larger applications, you can organize queries into multiple functions:

```python
# user_queries.py
from sqlty import sqlty

@sqlty
def user_sql[T](query: T) -> T:
    """User-related queries."""
    return query

# post_queries.py
from sqlty import sqlty

@sqlty
def post_sql[T](query: T) -> T:
    """Post-related queries."""
    return query
```

Use them in your application:

```python
# app.py
from user_queries import user_sql
from post_queries import post_sql
from sqlty.drivers.psycopg import execute

# Each function maintains its own type overloads
user_query = user_sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

users = execute(conn, user_query, {})
# Type: Iterator[tuple[int, str]]

post_query = post_sql("SELECT id::INTEGER, title::TEXT FROM posts")
# Type: SQL[tuple[int, str]]

posts = execute(conn, post_query, {})
# Type: Iterator[tuple[int, str]]
```

Generate stubs for multiple functions:

```bash
sqlty . --mode=full
# Detected @sqlty decorated function in 'user_queries', writing to user_queries.pyi
# Detected @sqlty decorated function in 'post_queries', writing to post_queries.pyi
```

The generator automatically detects multiple decorated functions and creates separate stub files for each.

### Runtime Auto-Generation

SQLTy can automatically generate and update stubs at runtime when you set the `SQLTY_AUTO_GENERATE` environment variable:

```bash
export SQLTY_AUTO_GENERATE=1
python your_app.py
```

When enabled:

1. **Function Definition**: Stubs are generated when a function is decorated with `@sqlty`
2. **Runtime Calls**: Additional `sql()` calls made during execution are tracked
3. **Exit Hook**: At script exit, all tracked calls are added to stubs using incremental mode
4. **File Locking**: Safe concurrent access in multi-threaded or multi-process environments

**Example workflow:**

```python
# development.py
import os
os.environ["SQLTY_AUTO_GENERATE"] = "1"

from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

# These calls are tracked and added to the stub at exit
user = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
posts = sql("SELECT COUNT(*)::INTEGER FROM posts")
```

Running this script will automatically create or update `development.pyi`:

```python
# development.pyi (generated automatically)
from typing import Literal, overload
from sqlty import SQL

@overload
def sql(query: Literal['SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id']) -> SQL[tuple[int, str]]: ...

@overload
def sql(query: Literal['SELECT COUNT(*)::INTEGER FROM posts']) -> SQL[tuple[int]]: ...

@overload
def sql[T](query: T) -> T: ...
```

**Use cases:**
- Development workflows with reload (e.g., running dev server with auto-reload)
- Running tests to ensure .pyi stubs are up-to-date with the codebase

**Performance:** The runtime registry uses caching and lazy initialization to minimize overhead. The auto-generation check is cached per decorated function, and frame inspection only happens when auto-generation is enabled. When `SQLTY_AUTO_GENERATE` is not set (default), there is negligible performance impact.

### Custom SQL Dialects

SQLTy supports multiple SQL dialects via sqlglot:

```bash
# PostgreSQL (default)
sqlty . --dialect postgres

# MySQL
sqlty . --dialect mysql

# SQLite
sqlty . --dialect sqlite

# BigQuery
sqlty . --dialect bigquery

# Snowflake
sqlty . --dialect snowflake
```

Example with MySQL:

```python
# Using MySQL-specific syntax
query = sql("""
    SELECT id, name, email
    FROM users
    WHERE id = ?
    LIMIT 10
""")
# Type: SQL[tuple[Any, Any, Any]] (no explicit casts)

result = execute(conn, query, (123,))
# Type: Iterator[tuple[Any, Any, Any]]
```

### Incremental Mode

Use incremental mode to add new overloads without removing existing ones:

```bash
# Add new overloads while preserving old ones
sqlty . --mode=incremental
```

This is useful for:
- Large codebases where full regeneration is slow
- Development workflows where you're adding queries iteratively
- Cases where some queries are in commented-out code

Use `--mode=full` mode to clean up stubs and remove queries that no longer exist:

```bash
# Remove overloads for queries that were deleted
sqlty . --mode=full
```

### Schemas and Precedence

Provide database schema files to improve type inference:

- Global schema for all registries:

```bash
sqlty . --schema schema.sql
```

- Registry-specific mappings (repeatable):

```bash
sqlty . \
  --schema users_queries:users.sql \
  --schema orders_queries:orders.sql
```

Precedence when choosing a schema for a registry:

1. Registry-specific mapping from `--schema registry:PATH`
2. Global mapping from `--schema PATH`
3. `__schema_path__` attribute on the registry function

CLI mappings, when present, always override `__schema_path__`.

### Custom Function and Export Names

You can customize the generated function and export names:

```bash
sqlty . \
  --function-name my_sql \
  --output my_queries.pyi
```

This generates:

```python
# my_queries.pyi
@overload
def my_sql(query: Literal["SELECT ..."]) -> SQL[tuple[int, str]]: ...

@overload
def my_sql[T](query: T) -> T: ...
```

### Specifying Output Path

Explicitly specify where to write the stub file:

```bash
# Single file mode
sqlty queries.py --output stubs/queries.pyi

# Directory mode with explicit output
sqlty . --output __init__.pyi
```

Without `--output`, the generator auto-detects output paths based on registry imports.

## Internals

### Architecture Overview

SQLTy consists of three main components:

1. **@sqlty Decorator**: Marks functions for type-safe SQL query execution
2. **TypeStubGenerator**: AST-based analyzer that collects SQL calls and generates stubs
3. **Type Inference Engine**: Uses sqlglot to parse SQL and infer Python types

```
┌─────────────────────────────────────────────────────────────┐
│                      Your Application                        │
│  ┌────────────┐         ┌──────────────┐                   │
│  │ queries.py │────────▶│ sql("...")   │                   │
│  └────────────┘         └──────────────┘                   │
└────────────────────────────┬────────────────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   TypeStubGenerator          │
              │  (AST Analysis)              │
              │                              │
              │  1. Parse Python AST         │
              │  2. Find sql() calls         │
              │  3. Extract SQL literals     │
              │  4. Detect registries        │
              └──────────┬───────────────────┘
                         │
                         ▼
              ┌──────────────────────────────┐
              │   SQL Type Inference         │
              │   (sqlglot)                  │
              │                              │
              │  1. Parse SQL                │
              │  2. Extract SELECT columns   │
              │  3. Map SQL types to Python  │
              └──────────┬───────────────────┘
                         │
                         ▼
              ┌──────────────────────────────┐
              │   Stub Generator             │
              │                              │
              │  1. Group by registry        │
              │  2. Deduplicate literals     │
              │  3. Generate overloads       │
              │  4. Write .pyi files         │
              └──────────────────────────────┘
```

### Type Inference

The type inference engine uses sqlglot to parse SQL queries and extract type information:

#### Column Type Inference

1. **Explicit Type Casts** (highest priority):
   ```sql
   SELECT id::INTEGER, name::TEXT FROM users
   ```
   Maps directly to `tuple[int, str]`

2. **Schema-Based Inference**:
   ```bash
   sqlty queries.py --schema schema.sql
   ```
   Uses database schema to infer column types when casts are not present.

3. **Fallback to Generic Types**:
   ```python
   SELECT * FROM users  -- Falls back to tuple[str, ...]
   ```
   Without casts or schema, infers generic string types.

#### Type Mapping

The inference engine maps SQL types to Python types:

```python
SQL_TYPE_MAP = {
    "INTEGER": "int",
    "BIGINT": "int",
    "SMALLINT": "int",
    "TEXT": "str",
    "VARCHAR": "str",
    "CHAR": "str",
    "BOOLEAN": "bool",
    "FLOAT": "float",
    "DOUBLE": "float",
    "REAL": "float",
    "DECIMAL": "Decimal",
    "NUMERIC": "Decimal",
    "DATE": "datetime.date",
    "TIMESTAMP": "datetime.datetime",
    "TIMESTAMPTZ": "datetime.datetime",
    "INTERVAL": "datetime.timedelta",
    "BYTEA": "bytes",
    "BLOB": "bytes",
    "UUID": "uuid.UUID",
}
```

#### Type Inference Priority

The generator tries multiple strategies in order of reliability:

1. **Explicit Type Casts in SQL** (most reliable):
   ```sql
   SELECT id::INTEGER, name::TEXT FROM users
   ```
   Directly maps SQL types to Python types.

2. **Schema Files**:
   ```bash
   sqlty queries.py --schema schema.sql
   ```
   Load database schema for enhanced type inference.

3. **Fallback to Generic Types**:
   ```python
   SELECT * FROM users  -- Falls back to tuple[str, ...]
   ```
   When type cannot be determined, infers generic string types.

**Best Practice:** Always use explicit type casts in SQL for reliable type inference:
```sql
SELECT
    id::INTEGER,
    name::TEXT,
    created_at::TIMESTAMP
FROM users
```

### AST Analysis

The generator uses Python's `ast` module to analyze source code:

#### Call Collection

The `LiteralCallCollector` visitor walks the AST and identifies `sql()` calls:

```python
class LiteralCallCollector(ast.NodeVisitor):
    def visit_Call(self, node: ast.Call) -> None:
        # Detect direct calls: sql("...")
        if isinstance(node.func, ast.Name):
            if node.func.id == self.function_name:
                self.is_target_call = True

        # Detect qualified calls: Query.sql("...")
        elif isinstance(node.func, ast.Attribute):
            if node.func.attr == self.function_name:
                self.is_target_call = True
                self.registry_source = self._get_registry_source(node.func)
```

#### Registry Detection

Registries are detected in two ways:

1. **Import-based detection**:
   ```python
   from queries import sql  # Detects 'queries' as registry source
   ```

2. **Function definition detection**:
   ```python
   @sqlty
   def sql[T](query: T) -> T:  # Detects 'sql' as decorated function
       return query
   ```

#### Source Location Tracking

Each `sql()` call is tracked with its source location:

```python
call_info = {
    "literal": "SELECT id::INTEGER FROM users",
    "file": "/path/to/app.py",
    "lineno": 42,
    "registry_source": "queries",
    "return_type": "SQL[tuple[int]]",
}
```

This information is used to generate source comments in stubs:

```python
# from app.py:42
@overload
@classmethod
def sql(cls, query: Literal["SELECT id::INTEGER FROM users"]) -> SQL[tuple[int]]: ...
```

### Stub Generation

#### Deduplication

The generator deduplicates SQL literals across files:

```python
# app.py
result1 = sql("SELECT id::INTEGER FROM users")

# handlers.py
result2 = sql("SELECT id::INTEGER FROM users")  # Same query

# Generated stub (only one overload):
@overload
@classmethod
def sql(cls, query: Literal["SELECT id::INTEGER FROM users"]) -> SQL[tuple[int]]: ...
```

#### Incremental Merging

In incremental mode, the generator merges new overloads with existing ones:

1. Parse existing `.pyi` file
2. Extract existing overloads
3. Add new overloads
4. Preserve fallback overload at the end

In full mode, the generator removes overloads that no longer exist in source code.

#### Fallback Overload

Every stub includes a generic fallback for non-literal queries:

```python
@overload
@classmethod
def sql[T](cls, query: T) -> T: ...
```

This ensures that dynamic queries still type-check:

```python
query_str = get_query_from_config()
result = sql(query_str)  # Type: str (falls back to T -> T)
```

## API Reference

### @sqlty Decorator

```python
from sqlty import sqlty
```

Decorator to mark functions for SQLTy type inference and stub generation.

#### Usage

```python
@sqlty
def sql[T](query: T) -> T:
    """SQL query function with type-safe overloads."""
    return query
```

#### Parameters

- `schema` (str, optional): Path to database schema file for enhanced type inference

#### Type Inference

The return type `T` is inferred from:
1. SQL type casts (e.g., `::INTEGER`)
2. Table schema (if available)
3. Usage patterns in code
4. Falls back to `Any` if unknown

#### Example

```python
@sqlty
def sql[T](query: T) -> T:
    return query

# Simple query
users = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]
```
# Type: SQL[tuple[int, str]]

# Parameterized query
user = sql("SELECT * FROM users WHERE id = :user_id")
# Type: SQL[tuple[Any, ...]]
```

### SQL Type

#### Usage

```python
from sqlty import SQL
```

Generic type representing SQL query results.

#### Type Parameters

- `T`: The tuple type representing row structure

#### Protocol

`SQL[T]` implements an iterable protocol:

```python
class SQL(Protocol[T]):
    def __iter__(self) -> Iterator[T]: ...
```

#### Usage

```python
from sqlty import SQL

def get_users() -> SQL[tuple[int, str]]:
    return sql("SELECT id::INTEGER, name::TEXT FROM users")

# Iterate over results
for user_id, name in get_users():
    print(f"User {user_id}: {name}")
```

### Generator CLI

```bash
sqlty <function_name> <source> [options]
```

Generate type stub files from SQL queries in source code.

#### Arguments

- `function_name`: Name of the function to generate overloads for (e.g., `sql`)
- `source`: Source file or directory to analyze

#### Options

##### `-o, --output PATH`

Output `.pyi` file path. If not specified, auto-detected based on registry imports.

```bash
sqlty queries.py --output stubs/queries.pyi
```

##### `--dialect DIALECT`

SQL dialect for query analysis. Default: `postgres`

Supported dialects:
- `postgres` (default)
- `mysql`
- `sqlite`
- `bigquery`
- `snowflake`
- `redshift`
- `oracle`
- `mssql`

```bash
sqlty . --dialect mysql
```

##### `--function-name NAME`

Name of the generated function. Default: `sql`

```bash
sqlty . --function-name my_query
```

##### `--mode=incremental`

Add new overloads to existing stub file instead of replacing it.

```bash
sqlty . --mode=incremental
```

##### `--mode=full`

Remove overloads for queries that no longer exist. This is the default for directory mode.

```bash
sqlty . --mode=full
```

#### Examples

**Generate stubs for a single file:**

```bash
sqlty app.py
```

**Generate stubs for a directory:**

```bash
sqlty src/ --mode=full
```

**Custom output path:**

```bash
sqlty . --output typings/queries.pyi
```

**Multiple functions:**

```bash
sqlty src/
# Detected @sqlty decorated function in 'user_queries', writing to src/user_queries.pyi
# Detected @sqlty decorated function in 'post_queries', writing to src/post_queries.pyi
```

**Incremental updates:**

```bash
# Initial generation
sqlty app.py

# Add new queries without removing old ones
sqlty app.py --mode=incremental
```

## Examples

### Example 1: Simple Query Registry

**File structure:**
```
project/
├── queries.py
├── app.py
└── schema.sql
```

**queries.py:**
```python
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    """Application SQL queries."""
    return query
```

**app.py:**
```python
from queries import sql
from sqlty.drivers.psycopg import execute

def get_user(user_id: int):
    query = sql("""
        SELECT id::INTEGER, name::TEXT, email::TEXT
        FROM users
        WHERE id = :user_id
    """)
    # Type: SQL[tuple[int, str, str]]

    result = execute(conn, query, {"user_id": user_id})
    # Type: Iterator[tuple[int, str, str]]

    return next(result, None)

def list_users():
    query = sql("SELECT id::INTEGER, name::TEXT FROM users ORDER BY name")
    # Type: SQL[tuple[int, str]]

    return execute(conn, query, {})
    # Type: Iterator[tuple[int, str]]
```

**Generate stubs:**
```bash
sqlty . --mode=full
mypy .
```

**Generated queries.pyi:**
```python
from typing import overload, Literal
from sqlty import SQL

# from queries.py:3
@overload
def sql(
    query: Literal["""
    SELECT id::INTEGER, name::TEXT, email::TEXT
    FROM users
    WHERE id = :user_id
"""],
) -> SQL[tuple[int, str, str]]: ...

# from queries.py:11
@overload
def sql(
    query: Literal["SELECT id::INTEGER, name::TEXT FROM users ORDER BY name"],
) -> SQL[tuple[int, str]]: ...

@overload
def sql[T](query: T) -> T: ...
```

### Example 2: Multi-Registry Application

**File structure:**
```
project/
├── user_queries.py
├── post_queries.py
└── app.py
```

**user_queries.py:**
```python
from sqlty import sqlty

@sqlty
def user_sql[T](query: T) -> T:
    """User-related queries."""
    return query
```

**post_queries.py:**
```python
from sqlty import sqlty

@sqlty
def post_sql[T](query: T) -> T:
    """Post-related queries."""
    return query
```

**app.py:**
```python
from user_queries import user_sql
from post_queries import post_sql
from sqlty.drivers.psycopg import execute

def get_user_with_posts(user_id: int):
    user_query = user_sql("""
        SELECT id::INTEGER, name::TEXT, email::TEXT
        FROM users WHERE id = :user_id
    """)
    # Type: SQL[tuple[int, str, str]]

    user = execute(conn, user_query, {"user_id": user_id})
    # Type: Iterator[tuple[int, str, str]]

    posts_query = post_sql("""
        SELECT id::INTEGER, title::TEXT, created_at::TIMESTAMP
        FROM posts WHERE user_id = :user_id
    """)
    # Type: SQL[tuple[int, str, datetime]]

    posts = execute(conn, posts_query, {"user_id": user_id})
    # Type: Iterator[tuple[int, str, datetime]]

    return user, posts
```

**Generate stubs:**
```bash
sqlty . --mode=full
# Detected @sqlty decorated function in 'user_queries', writing to user_queries.pyi
# Detected @sqlty decorated function in 'post_queries', writing to post_queries.pyi
```

This creates separate stub files for each registry, keeping types organized.

## Contributing

### Development Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/jirikuncar/sqlty.git
   cd sqlty
   ```

2. Install dependencies:
   ```bash
   uv pip install -e ".[dev]"
   ```

3. Run tests:
   ```bash
   pytest tests/
   ```

4. Type check:
   ```bash
   mypy src/
   ```

### Running Examples

Test the examples to ensure everything works:

```bash
make verify-examples
```

### Code Style

This project uses:
- `ruff` for linting and formatting
- `mypy` for type checking (strict mode)
- `pytest` for testing

### Submitting Changes

1. Create a feature branch
2. Make your changes
3. Add tests
4. Ensure all tests pass
5. Submit a pull request

## License

MIT License - see LICENSE file for details.

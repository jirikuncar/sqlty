# Function Configuration Pattern - Complete Guide

## Overview

The function configuration pattern allows you to configure existing functions for SQL type inference via `pyproject.toml`, without modifying their source code. This is ideal for database helper functions, third-party libraries, or legacy code.

## Basic Configuration

### Configuration Structure

In your `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "module.function_name"  # Full import path
query_param = "query"                 # Parameter containing SQL query
return_type_param = "return_tp"       # (Optional) Parameter with return type
schema = "schema.sql"                 # (Optional) Schema file path
```

### Required Fields

- **`import_path`**: Full import path to the function (e.g., `"db.pg_fetchvalue"`)
- **`query_param`**: Name of the parameter that contains the SQL query string

### Optional Fields

- **`return_type_param`**: Name of the parameter that specifies the expected return type
- **`schema`**: Path to SQL schema file for enhanced type inference

## Usage Patterns

### Pattern 1: Function with Explicit Return Type

Configure a function that accepts a return type parameter:

```toml
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
```

Usage in code:

```python
from db import pg_fetchvalue

# Type determined by return_tp parameter
user_id = pg_fetchvalue(
    conn,
    query="SELECT id::INTEGER FROM users WHERE email = :email",
    return_tp=int,
    email="user@example.com"
)
# Type: int | None
```

Generated stub:

```python
@overload
def pg_fetchvalue(
    query: Literal["SELECT id::INTEGER FROM users WHERE email = :email"]
) -> int: ...
```

### Pattern 2: Function Without Return Type Parameter

Configure a function that infers types from the SQL query:

```toml
[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
# No return_type_param - types inferred from SQL
```

Usage in code:

```python
from db import execute_query

results = execute_query(
    query="SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id",
    id=1
)
# Type inferred from SQL: list[tuple[int, str]]
```

### Pattern 3: Schema-Based Type Inference

Add a schema file for enhanced type inference:

```toml
[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"  # Enable schema-based inference
```

With schema.sql:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL
);
```

Usage:

```python
# Types inferred from schema definition
results = execute_query(
    query="SELECT id, name FROM users WHERE id = :id",
    id=1
)
# Type: list[tuple[int, str]]
```

## Type Inference Rules

### With `return_type_param`

When configured with a `return_type_param`:

1. SQLTy extracts the value passed to that parameter
2. Uses it as the return type in generated stubs
3. SQL query structure is validated but not used for typing

Example:

```python
# return_tp=str determines the type
value = pg_fetchvalue(
    conn,
    query="SELECT COUNT(*)::INTEGER FROM users",  # Returns INTEGER
    return_tp=str,  # But user expects str
)
# Type: str | None (potential mismatch - mypy won't catch this)
```

### Without `return_type_param`

When no `return_type_param` is configured:

1. SQLTy parses the SQL query structure
2. Infers types from explicit casts (e.g., `::INTEGER`)
3. Falls back to schema definitions if available

Example:

```python
# Types inferred from SQL casts
results = execute_query(
    query="SELECT id::INTEGER, name::TEXT FROM users"
)
# Type: list[tuple[int, str]]
```

### With Schema

When a schema is provided:

1. SQLTy loads and parses the schema file
2. Uses table/column definitions for type inference
3. Validates query references against schema
4. Infers types even without explicit casts

Example with schema:

```python
# Schema defines: id INTEGER, name TEXT
results = execute_query(
    query="SELECT id, name FROM users"  # No explicit casts needed
)
# Type: list[tuple[int, str]] (from schema)
```

## Stub Generation

Generate type stubs with the `sqlty` command:

```bash
# Generate stubs for current directory
sqlty .

# Generate stubs for specific path
sqlty /path/to/project
```

This creates `.pyi` stub files with type-safe overloads:

```python
# db.pyi (generated)
from typing import Literal, overload

@overload
def pg_fetchvalue(
    query: Literal["SELECT id::INTEGER FROM users WHERE email = :email"]
) -> int: ...

@overload
def pg_fetchvalue(
    query: Literal["SELECT COUNT(*)::INTEGER FROM users"]
) -> int: ...

@overload
def pg_fetchvalue(query: str) -> str: ...
```

## Multiple Function Configurations

You can configure multiple functions in one `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgresql"

# Function with return type parameter
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
schema = "schema.sql"

# Function without return type parameter
[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"

# Third-party library function
[[tool.sqlty.functions]]
import_path = "external.lib.sql_helper"
query_param = "sql_query"
return_type_param = "expected_type"
```

## Mypy Integration

After generating stubs, run mypy for type checking:

```bash
mypy app.py
```

Mypy will:
- Use generated stubs for type inference
- Validate query literals against overloads
- Check return types match variable annotations
- Provide IDE autocomplete support

## Advanced Features

### Keyword Arguments

Functions must use keyword arguments for the query parameter:

```python
# ✓ Works - keyword argument
result = pg_fetchvalue(conn, query="SELECT id FROM users", return_tp=int)

# ✗ Doesn't work - positional argument
result = pg_fetchvalue(conn, "SELECT id FROM users", int)
```

### Parameter Name Matching

The `query_param` and `return_type_param` names must exactly match the function signature:

```python
def pg_fetchvalue(conn, query: str, return_tp=None, **params):
    #                      ^^^^^       ^^^^^^^^^
    #                      |           |
    #                      |           return_type_param="return_tp"
    #                      query_param="query"
```

### Schema Path Resolution

Schema paths are relative to the `pyproject.toml` directory:

```toml
# If pyproject.toml is in /project/pyproject.toml
[[tool.sqlty.functions]]
schema = "schema.sql"           # Resolves to /project/schema.sql
schema = "schemas/users.sql"    # Resolves to /project/schemas/users.sql
```

## Benefits

1. **No Code Modification**: Works with existing functions without decorators
2. **Third-Party Support**: Can configure library functions you don't control
3. **Legacy Code**: Add type safety to old codebases without refactoring
4. **Clean Separation**: Configuration separate from implementation
5. **Schema Integration**: Optional schema support for better inference
6. **Flexible**: Works with any function signature pattern

## Comparison with Decorator Pattern

### Function Config Pattern (This Example)

**Pros:**
- No code changes required
- Works with third-party libraries
- Configuration in one place
- Can configure multiple functions

**Cons:**
- Configuration separate from code
- Requires pyproject.toml
- Less obvious which functions are tracked

**Example:**
```python
# db.py (unchanged)
def pg_fetchvalue(conn, query, return_tp=None, **params):
    return execute(conn, query, params)

# pyproject.toml
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
```

### Decorator Pattern

**Pros:**
- Inline configuration
- Clear marking of SQL functions
- Runtime attributes available
- More explicit

**Cons:**
- Requires modifying source code
- Not suitable for third-party libraries
- Decorator syntax required

**Example:**
```python
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query
```

## Common Patterns

### Pattern: Database Helper Library

Create a reusable database helper module:

```python
# db_helpers.py
def fetch_one(query: str, return_type=None, **params):
    """Fetch single value."""
    pass

def fetch_all(query: str, **params):
    """Fetch all rows."""
    pass

def execute(query: str, **params):
    """Execute query without return."""
    pass
```

Configure in `pyproject.toml`:

```toml
[[tool.sqlty.functions]]
import_path = "db_helpers.fetch_one"
query_param = "query"
return_type_param = "return_type"

[[tool.sqlty.functions]]
import_path = "db_helpers.fetch_all"
query_param = "query"

[[tool.sqlty.functions]]
import_path = "db_helpers.execute"
query_param = "query"
```

### Pattern: Third-Party Library

Configure external library functions:

```python
# Using asyncpg library (example)
import asyncpg

# Configure in pyproject.toml
[[tool.sqlty.functions]]
import_path = "asyncpg.Connection.fetchval"
query_param = "query"
```

## Testing

Test your configuration by:

1. Running the stub generator:
   ```bash
   sqlty .
   ```

2. Checking generated stubs:
   ```bash
   cat db.pyi
   ```

3. Running mypy:
   ```bash
   mypy app.py
   ```

4. Verifying IDE autocomplete works

## Troubleshooting

### Issue: Stubs not generated

**Check:**
- Is `pyproject.toml` in the correct location?
- Is the `import_path` correct and importable?
- Are functions called with keyword arguments?

### Issue: Wrong types inferred

**Solutions:**
- Add explicit type casts in SQL (e.g., `::INTEGER`)
- Provide a schema file for better inference
- Use `return_type_param` for explicit typing

### Issue: Mypy doesn't find overloads

**Solutions:**
- Ensure `.pyi` files are generated
- Check mypy can find the stub files
- Verify import paths match

## Examples

See the complete example in `examples/function_config/` with:
- Database helper functions
- Application code
- Configuration file
- Schema file
- Generated stubs

Run the example:

```bash
cd examples/function_config
sqlty .
mypy app.py
```

## Related Documentation

- [Decorator Pattern Guide](../decorator_pattern/GUIDE.md)
- [Schema-Based Pattern](../schema_based/)
- [Main SQLTy Documentation](../../README.md)

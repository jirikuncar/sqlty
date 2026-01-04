# Function Configuration Example

This example demonstrates how to configure functions for SQLTy analysis via `pyproject.toml` rather than using `@sqlty` decorators. This is particularly useful for:

- Database helper functions you don't want to modify
- Third-party library functions
- Legacy codebases where you want type safety without code changes

## Features

- **Configuration-based detection**: Functions are marked for analysis via `pyproject.toml`
- **No code modification needed**: Works with existing functions without decorators
- **Schema support**: Optional schema files for enhanced type inference
- **Flexible parameters**: Support for functions with custom parameter names

## Structure

```
function_config/
├── app.py              # Application code using configured functions
├── db.py               # Database utility functions (no decorators needed)
├── pyproject.toml      # SQLTy configuration
├── schema.sql          # Database schema for type inference
└── README.md          # This file
```

## Configuration

The `pyproject.toml` file configures which functions SQLTy should analyze:

```toml
[tool.sqlty]
dialect = "postgresql"

# Function with explicit return type parameter
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
schema = "schema.sql"

# Function without return type parameter (infers from SQL)
[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"
```

### Configuration Fields

- `import_path`: Full import path to the function (e.g., `"module.submodule.function_name"`)
- `query_param`: Name of the parameter containing the SQL query string
- `return_type_param`: (Optional) Name of the parameter specifying the expected return type
- `schema`: (Optional) Path to SQL schema file for type inference

## Usage

1. **Generate type stubs:**
   ```bash
   sqlty .
   ```

   This will create `db.pyi` with type-safe overloads for your database functions.

2. **Check types with mypy:**
   ```bash
   mypy app.py
   ```

   Mypy will use the generated stubs to provide type checking and IDE autocomplete.

## How It Works

### With `return_type_param`

When a function has a `return_type_param` configured, SQLTy uses the value passed to that parameter:

```python
user_id = pg_fetchvalue(
    conn,
    query="SELECT id FROM users WHERE email = :email",
    return_tp=int,  # This determines the return type
    email="user@example.com"
)
# Type: int | None
```

Generated stub:
```python
@overload
def pg_fetchvalue(
    query: Literal["SELECT id FROM users WHERE email = :email"]
) -> int: ...
```

### Without `return_type_param`

When no `return_type_param` is configured, SQLTy infers types from the SQL query:

```python
results = execute_query(
    query="SELECT id::INTEGER, name::TEXT FROM users",
)
# Type: list[tuple[int, str]]
```

### Schema-Based Type Inference

When a `schema` is provided, SQLTy can validate and enhance type inference:

```python
# Schema defines: CREATE TABLE users (id INTEGER, name TEXT, ...)
results = execute_query(
    query="SELECT id, name FROM users WHERE id = :id",
    id=1
)
# Types inferred from schema: list[tuple[int, str]]
```

## Benefits

1. **No code changes**: Works with existing functions
2. **Type safety**: Full mypy type checking for SQL queries
3. **IDE support**: Autocomplete and type hints in your editor
4. **Clean separation**: Configuration separate from implementation
5. **Flexible**: Works with any function signature

## Comparison with Decorator Pattern

### Function Config (This Example)
```python
# db.py - no decorators needed
def pg_fetchvalue(conn, query: str, return_tp=None, **params):
    return execute(conn, query, params)

# pyproject.toml
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
```

### Decorator Pattern
```python
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

result = sql("SELECT id FROM users")
```

Both approaches are valid and can be used together. Choose based on:
- Use function config when you can't or don't want to modify source code
- Use decorators when you're creating new code and want inline configuration

## Next Steps

After generating stubs, you can:
- Run `mypy app.py` to check for type errors
- Use your IDE's autocomplete to explore available overloads
- Add more SQL queries and regenerate stubs
- Extend the schema for better type inference

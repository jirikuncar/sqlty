# Decorator Pattern Example

This example demonstrates the `@sqlty` decorator pattern for marking functions for SQL type inference and stub generation.

## Features

- **Simple decorator**: `@sqlty` marks a function for detection
- **Schema support**: `@sqlty(schema="schema.sql")` associates a schema file
- **Automatic attributes**: Functions get `__sqlty__` and `__schema_path__` attributes
- **Stub generation**: The `sqlty` tool detects decorated functions and generates type stubs
- **pyproject.toml registration**: When `SQLTY_AUTO_GENERATE=1`, functions must be registered

## Configuration

### pyproject.toml

```toml
[tool.sqlty]
dialect = "postgres"

# Register the @sqlty decorated functions
[[tool.sqlty.functions]]
import_path = "app.sql"
query_param = "query"

[[tool.sqlty.functions]]
import_path = "app.sql_with_schema"
query_param = "query"
schema = "schema.sql"
```

## Usage

```python
from typing import Any
from sqlty import SQL, sqlty

@sqlty
def sql(query: str) -> Any:
    """Simple SQL function decorated with @sqlty."""
    return query

@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    """SQL function with schema path for schema-based type inference."""
    return query

# Use the functions
q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
q2: SQL[tuple[int, str]] = sql_with_schema("SELECT id::INTEGER, name::TEXT FROM users")
```

## Running the Example

1. Run the example:
```bash
python app.py
```

2. Generate stub file:
```bash
sqlty . --mode=full
```

3. Check the generated `app.pyi` file for type-safe overloads

## Notes

- The `@sqlty` decorator marks functions for SQLTy to track
- When `SQLTY_AUTO_GENERATE=1` is set, the decorator will check that functions are registered in pyproject.toml
- Stub generation happens via the CLI tool or when properly configured with the environment variable

# Decorator Pattern Example

This example demonstrates the new `@sqlty` decorator pattern for marking functions for SQL type inference and stub generation.

## Features

- **Simple decorator**: `@sqlty` marks a function for detection
- **Schema support**: `@sqlty(schema="schema.sql")` associates a schema file
- **Automatic attributes**: Functions get `__sqlty__` and `__schema_path__` attributes
- **Stub generation**: The `sqlty` tool detects decorated functions and generates type stubs

## Usage

1. Run the example:
```bash
python app.py
```

2. Generate stub file:
```bash
sqlty . --mode=full
```

3. Check the generated `app.pyi` file for type-safe overloads

## Comparison with Registry Pattern

Traditional registry pattern:
```python
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
```

New decorator pattern:
```python
from typing import Any
from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    return query
```

Both patterns are supported and can be used interchangeably.

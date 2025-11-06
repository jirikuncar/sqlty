# @sqlty Decorator Pattern

This example demonstrates the recommended pattern for using `sqlty` with the `@sqlty` decorator.

## Overview

Instead of generating stubs for individual files, we create a central `sql` function decorated with `@sqlty`. This function serves as the registry for all SQL queries in your application.

## Files

- `queries.py`: Defines the `sql` function decorated with `@sqlty`
- `queries.pyi`: Generated stub file with type-safe overloads for all queries
- `query.py`: Example usage of the `sql` function

## How It Works

1. **Define the SQL function** (`queries.py`):
```python
from typing import Any
from sqlty import sqlty

@sqlty
def sql(query: str) -> Any:
    """Execute SQL queries."""
    return query
```

2. **Generate the stub** with overloads:
```bash
uv run sqlty query.py --check
```

This generates `queries.pyi` with type-safe overloads:
```python
@overload
def sql(
    query: Literal["""SELECT id, name FROM users WHERE id = :id"""],
) -> SQL[tuple[int, str]]: ...

# ... more overloads ...

@overload
def sql[T](query: T) -> T: ...
```

3. **Use in your code**:
```python
from queries import sql
from sqlty.drivers.psycopg import execute

query = sql("""
    SELECT id, name FROM users WHERE id = :id
    """)

rows: Iterator[tuple[int, str]] = execute(conn, query, params)
```

## Benefits

- ✅ **Simple and direct**: Just decorate a function with `@sqlty`
- ✅ **Stubs stay with your application code**, not scattered across the codebase
- ✅ **mypy correctly picks up the stubs** when checking files that import from `queries`
- ✅ **Type-safe**: Unknown queries fall back to returning `str`, which causes type errors
- ✅ **Centralized**: All query types in one place
- ✅ **Scalable**: Works for any number of queries across multiple files

## Regenerating Stubs

When you add new queries, regenerate the stub:

```bash
# Analyze all .py files in the current directory
uv run sqlty . --check

# Or analyze specific files
uv run sqlty query.py other_file.py --check
```

## Why This Works

Per the [mypy stubs documentation](https://mypy.readthedocs.io/en/stable/stubs.html):

- Stub files (`.pyi`) are used when mypy checks code that **imports** from that module
- When you run `mypy query.py`, it checks `query.py` which imports `sql` from `queries`
- mypy uses `queries.pyi` to type-check the `sql()` calls in `query.py`
- This is the standard pattern for type stubs in Python (PEP 561)

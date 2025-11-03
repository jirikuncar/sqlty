# Query Registry Pattern

This example demonstrates the recommended pattern for using `sqltype` with the `SQLRegistry` base class.

## Overview

Instead of generating stubs for individual files, we create a central `Query` class that inherits from `SQLRegistry`. This class serves as a registry for all SQL queries in your application.

## Files

- `queries.py`: Defines the `Query` class and exports the `sql` function
- `queries.pyi`: Generated stub file with type-safe overloads for all queries
- `query.py`: Example usage of the `sql` function

## How It Works

1. **Define the Query registry** (`queries.py`):
```python
from sqltype import SQLRegistry

class Query(SQLRegistry):
    """Application-specific SQL query registry."""
    pass

sql = Query.sql
```

2. **Generate the stub** with overloads:
```bash
uv run sqlty query.py --class -o queries.pyi
```

This generates `queries.pyi` with type-safe overloads:
```python
class Query(SQLRegistry):
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["""SELECT id, name FROM users WHERE id = :id"""],
    ) -> SQL[tuple[int, str]]: ...

    # ... more overloads ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...
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

- ✅ **Stubs stay with your application code**, not scattered across the codebase
- ✅ **mypy correctly picks up the stubs** when checking files that import from `queries`
- ✅ **Type-safe**: Unknown queries fall back to returning `str`, which causes type errors
- ✅ **Centralized**: All query types in one place
- ✅ **Scalable**: Works for any number of queries across multiple files

## Regenerating Stubs

When you add new queries, regenerate the stub:

```bash
# Analyze all .py files in the current directory
uv run sqlty . --class -o queries.pyi

# Or analyze specific files
uv run sqlty query.py other_file.py --class -o queries.pyi
```

## Why This Works

Per the [mypy stubs documentation](https://mypy.readthedocs.io/en/stable/stubs.html):

- Stub files (`.pyi`) are used when mypy checks code that **imports** from that module
- When you run `mypy query.py`, it checks `query.py` which imports `sql` from `queries`
- mypy uses `queries.pyi` to type-check the `sql()` calls in `query.py`
- This is the standard pattern for type stubs in Python (PEP 561)

# @sqlty Decorator Pattern - Complete Guide

## Overview

The `@sqlty` decorator provides an alternative pattern to the `SQLRegistry` class for marking functions for SQL type inference and stub generation. This pattern offers a simpler, more direct approach while maintaining all the power of the registry system.

## Basic Usage

### Simple Decorator

```python
from typing import Any
from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    return query

# Usage
result = sql("SELECT 1::INTEGER")
```

At runtime, decorated functions have the `__sqlty__` attribute:
```python
assert sql.__sqlty__ is True
```

### Decorator with Schema

```python

@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    return query
```

This function has both attributes:
```python
assert sql_with_schema.__sqlty__ is True
assert sql_with_schema.__schema_path__ == "schema.sql"
```

## Type Safety

Use with TYPE_CHECKING for type annotations:

```python
from typing import TYPE_CHECKING, Any
from sqlty import sqlty

if TYPE_CHECKING:
    from sqlty import SQL


@sqlty
def sql(query: str) -> Any:
    return query

# Type-safe usage
q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
```

## Stub Generation

Generate type stubs with the `sqlty` command:

```bash
# Generate stubs for current directory
sqlty . --mode=full

# Generate stubs for specific file
sqlty app.py --mode=full
```

This creates an `app.pyi` file with `@overload` declarations:

```python
from typing import Any, Literal, overload
from sqlty import SQL


@overload
def sql(query: Literal['SELECT 1::INTEGER']) -> SQL[tuple[int]]:
    ...
@overload
def sql(query: str) -> Any:
    ...
```

## Schema Integration

When using `@sqlty(schema="...")`, the stub generator will:
1. Load the schema file
2. Use it for enhanced type inference
3. Infer types from table definitions

Example:

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

    return query

@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    return query


# With schema, types can be inferred even without explicit casts
if TYPE_CHECKING:
    q: SQL[tuple[int, str]] = sql_with_schema("SELECT id, name FROM users")
```

## Comparison with Registry Pattern

### Registry Pattern (Original)

```python
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql

# Usage
q = sql("SELECT 1")
```

### Decorator Pattern (New)

```python
from sqlty import sqlty

@sqlty
def sql(query: str) -> Any:
    return query

# Usage
q = sql("SELECT 1")
```

Both patterns:
- Generate the same stub files
- Provide the same type safety
- Work with mypy
- Support schema integration

## Multiple Functions

You can have multiple decorated functions in one file:

```python
@sqlty
def sql[T](query: T) -> T:
    return query

@sqlty(schema="users.sql")
def user_sql[T](query: T) -> T:
    return query

@sqlty(schema="orders.sql")
def order_sql[T](query: T) -> T:
    return query
```

Each function gets its own set of overloads in the stub file.

## Mypy Integration

After generating stubs, mypy will type-check your code:

```bash
mypy app.py
```

Mypy will:
- Validate query literals match generated overloads
- Check return types match variable annotations
- Catch type mismatches at compile time

## Benefits

1. **Simpler Syntax**: No need to create a class
2. **Direct Usage**: Functions are directly callable
3. **Clear Intent**: Decorator clearly marks SQL functions
4. **Schema Association**: Easy schema path specification
5. **Runtime Introspection**: `__sqlty__` and `__schema_path__` attributes

## When to Use Each Pattern

### Use Decorator Pattern When:
- You have simple SQL query functions
- You want minimal boilerplate
- You prefer function-based APIs
- Each function has a different schema

### Use Registry Pattern When:
- You need class-based organization
- You want to group multiple query methods
- You prefer the class export pattern
- You have complex query hierarchies

## Implementation Details

### Decorated Function Attributes

- `__sqlty__`: Always `True` for decorated functions
- `__schema_path__`: Set if `schema` parameter provided

### Detection

The SQLTy analyzer:
1. Parses Python AST to find `@sqlty` decorators
2. Tracks decorated function names
3. Associates calls with their decorated functions
4. Extracts schema paths from decorator arguments

### Stub Structure

Generated stubs contain:
- Multiple `@overload` declarations (one per unique query)
- A generic fallback `@overload` with type variable
- Function signatures (not class methods)
- No `@classmethod` decorator
- No `cls` parameter

## Examples

See `examples/decorator_pattern/` for a complete working example with:
- Simple and schema-based decorators
- Schema file
- Generated stub file
- README with instructions

## Testing

The e2e test suite in `tests/e2e/test_decorator_pattern.py` covers:
- Simple decorator pattern
- Decorator with schema parameter
- Multiple decorated functions
- Runtime attribute verification
- Stub generation validation
- Mypy integration

Run tests with:
```bash
pytest tests/e2e/test_decorator_pattern.py -v
```

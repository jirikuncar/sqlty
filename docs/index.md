# SQLTy User Guide

Type-safe SQL queries for Python with automatic type inference, source rewriting, and lint checks.

## Installation

```bash
pip install sqlty
# or
uv add sqlty
```

Requires Python 3.15+. See [installation guide](installation.md) for details.

## Quick Start

**1. Write SQL with `--sql\n` prefix and type casts:**

```python
# queries.py
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
```

**2. Rewrite to add type annotations:**

```bash
sqlty rewrite queries.py
```

Result:

```python
# queries.py
from sqlty import SQL

query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

**3. Check for issues:**

```bash
sqlty check queries.py
```

**4. Strip annotations (reverse):**

```bash
sqlty strip queries.py
```

## Writing SQL Queries

Use the `--sql\n` prefix to mark SQL strings. The `\n` separates the marker from the actual SQL (so the marker is a SQL comment, not part of the query):

```python
# Single-line (escaped newline)
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"

# Multi-line (natural newline)
query = """--sql
SELECT id::INTEGER, name::TEXT, email::TEXT
FROM users
WHERE active = true
"""
```

### Type Casts

Use PostgreSQL-style `::TYPE` casts to declare column types:

| SQL Cast | Python Type | Import |
|----------|-------------|--------|
| `::INTEGER` | `int` | Built-in |
| `::TEXT`, `::VARCHAR` | `str` | Built-in |
| `::BOOLEAN` | `bool` | Built-in |
| `::FLOAT`, `::DOUBLE` | `float` | Built-in |
| `::DATE` | `datetime.date` | `import datetime` |
| `::TIMESTAMP` | `datetime.datetime` | `import datetime` |
| `::TIME` | `datetime.time` | `import datetime` |
| `::INTERVAL` | `datetime.timedelta` | `import datetime` |
| `::UUID` | `uuid.UUID` | `import uuid` |
| `::JSON`, `::JSONB` | `Any` | `from typing import Any` |
| `::BYTEA` | `bytes` | Built-in |

### Schema-Based Inference

Skip explicit casts by providing a schema file:

```toml
# pyproject.toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

No casts needed — types inferred from schema:

```python
query = "--sql\nSELECT id, name, email FROM users"
# rewritten: query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

## User-Defined Types

Beyond `tuple[...]`, sqlty validates `TypedDict`, `NamedTuple`, `dataclass`, and Pydantic `BaseModel` annotations against query columns.

### Basic example

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# sqlty check validates field count, types, and names
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Supported type forms

| Type Form | Example |
|-----------|---------|
| `TypedDict` | `class User(TypedDict): id: int; name: str` |
| `NamedTuple` | `class User(NamedTuple): id: int; name: str` |
| `@dataclass` | `@dataclass class User: id: int; name: str` |
| `BaseModel` | `class User(BaseModel): id: int; name: str` |

### What gets checked

- **Field count**: `User` has 3 fields but query returns 2 columns
- **Field types**: `User.id` is `str` but query infers `int`
- **Field names**: `User.user_id` doesn't match column `id`

### Import resolution

SQLTy uses Python's runtime typing machinery to resolve types. This means `from datetime import datetime` correctly matches `TIMESTAMP` columns — no false positives from import style differences:

```python
from datetime import datetime  # not datetime.datetime — sqlty handles both
from typing import TypedDict
from sqlty import SQL

class Event(TypedDict):
    id: int
    created_at: datetime  # resolves to datetime.datetime at runtime

query = SQL[Event]("--sql\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
# No type mismatch — runtime resolution handles it correctly
```

### Limitations

- Type must be defined in the **same file** as the `SQL[T](...)` usage
- Cross-module imports are silently skipped

## CLI Commands

```
sqlty <COMMAND> [OPTIONS] [PATH...]

Commands:
  rewrite     "--sql\n..." → SQL[T]("--sql\n...")
  strip       SQL[T]("--sql\n...") → "--sql\n..."
  check       Lint type annotations, detect issues
  version     Print version
```

### `sqlty rewrite`

```bash
sqlty rewrite .                    # rewrite all Python files
sqlty rewrite --check .            # dry run — exit 1 if changes needed
sqlty rewrite --diff .             # show diff without applying
sqlty rewrite --schema schema.sql  # with schema for enhanced inference
```

### `sqlty strip`

```bash
sqlty strip .                      # strip all wrappers
sqlty strip --check .              # exit 1 if wrappers found
sqlty strip --diff .               # show diff
```

### `sqlty check`

```bash
sqlty check .                      # run all lint checks
sqlty check --schema schema.sql .  # with schema for column validation
```

### Common options

```bash
--dialect <DIALECT>     SQL dialect: postgres (default), sqlite, mysql
--schema <PATH>         Path to schema .sql file
-v, --verbose           Increase verbosity
-q, --quiet             Suppress non-error output
```

## Configuration

Configure via `[tool.sqlty]` in `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

CLI flags override config values. See [API reference](api.md) for full configuration.

## Magic Codec

Import-time transformation — no `sqlty rewrite` step needed:

```python
# coding: sqlty
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: # coding: sqlty
# decoded: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

See [magic codec documentation](magic-codec.md) for setup.

## Next Steps

- **[Usage Patterns](patterns.md)** — Which workflow is right for you
- **[API Reference](api.md)** — Full CLI and configuration reference
- **[Advanced Usage](advanced-usage.md)** — Dialects, schemas, pre-commit

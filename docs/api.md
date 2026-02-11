# API Reference

Complete API documentation for SQLTy.

## CLI

### Command Line Interface

```
sqlty <COMMAND> [OPTIONS] [PATH...]
```

### Commands

#### `sqlty rewrite`

Rewrite `"--sql ..."` strings into `SQL[T]("--sql\n...")` calls with inferred types.

```bash
sqlty rewrite [OPTIONS] [PATH...]
```

**Options:**
- `--check` — Exit 1 if files need rewriting (don't modify files)
- `--diff` — Show diff without applying changes
- `--dialect <DIALECT>` — SQL dialect: `postgres` (default), `sqlite`, `mysql`
- `--schema <PATH>` — Path to schema `.sql` file for enhanced inference
- `-v, --verbose` — Increase verbosity
- `-q, --quiet` — Suppress non-error output

**Examples:**
```bash
# Rewrite all Python files
sqlty rewrite .

# Dry run
sqlty rewrite --check .

# With schema
sqlty rewrite --schema schema.sql src/
```

#### `sqlty strip`

Strip `SQL[T](...)` wrappers back to plain `"--sql ..."` strings.

```bash
sqlty strip [OPTIONS] [PATH...]
```

**Options:**
- `--check` — Exit 1 if files contain `SQL[T](...)` wrappers
- `--diff` — Show diff without applying changes
- `-v, --verbose` — Increase verbosity
- `-q, --quiet` — Suppress non-error output

**Examples:**
```bash
# Strip all wrappers
sqlty strip .

# Check if any files need stripping
sqlty strip --check .
```

#### `sqlty check`

Lint SQL type annotations and detect issues.

```bash
sqlty check [OPTIONS] [PATH...]
```

**Options:**
- `--dialect <DIALECT>` — SQL dialect: `postgres` (default), `sqlite`, `mysql`
- `--schema <PATH>` — Path to schema `.sql` file
- `-v, --verbose` — Increase verbosity
- `-q, --quiet` — Suppress non-error output

**Examples:**
```bash
# Check all files
sqlty check .

# With schema for column validation
sqlty check --schema schema.sql .
```

**Exit codes:**
- `0` — No errors found
- `1` — One or more errors detected

#### `sqlty version`

Print version and exit.

---

## Core Types

### `SQL[T]`

```python
from sqlty import SQL
```

Generic type representing typed SQL query strings. `SQL` is a `str` subclass parameterized by the inferred return type:

```python
class SQL[T](str):
    pass
```

**Usage:**

```python
from sqlty import SQL

# Created by `sqlty rewrite`
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

---

## Configuration

### `pyproject.toml`

SQLTy is configured via `[tool.sqlty]` in `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgres"           # SQL dialect
schema = "schema.sql"          # Path to schema file
return-format = "tuple"        # Return type format
format-command = "ruff format --stdin-filename {filename} -"

[tool.sqlty.schemas]           # Named schema registry
main = "db/schema.sql"
analytics = "db/analytics.sql"

[tool.sqlty.rules]             # Per-rule severity
column-count-mismatch = "error"
column-type-mismatch = "error"
column-name-mismatch = "warn"
select-star = "warn"
select-star-no-schema = "error"
stale-rewrite = "warn"
stale-type = "warn"
```

CLI flags override config file values.

---

## Type Mappings

### SQL to Python Type Mapping

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

---

## Lint Rules

| Rule | Default | Fixable | Description |
|------|---------|---------|-------------|
| `column-count-mismatch` | error | no | Annotation has different field count than query columns |
| `column-type-mismatch` | error | no | Annotation field type differs from inferred type |
| `column-name-mismatch` | warn | no | Annotation field name differs from column name |
| `select-star` | warn | yes | `SELECT *` used — expand to explicit column list |
| `select-star-no-schema` | error | no | `SELECT *` used but no schema available |
| `stale-rewrite` | warn | yes | `--sql` strings not wrapped in `SQL[T](...)` |
| `stale-type` | warn | yes | Type annotation doesn't match current inference |

Configure severity in `pyproject.toml`:

```toml
[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

---

## Deferred Features

The following features are documented in [PLAN-FUTURE.md](../PLAN-FUTURE.md):

- **Stub generation** (`sqlty generate`) — Generate `.pyi` overload stubs from configured functions
- **Structured return types** — Protocol-based typing with TypedDict/namedtuple/Pydantic models

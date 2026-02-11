# SQLTy Documentation

Type-safe SQL queries for Python with automatic type inference, source rewriting, and lint checks.

## Table of Contents

- [Motivation](motivation.md)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Basic Usage](#basic-usage)
  - [Writing SQL Queries](#writing-sql-queries)
  - [Type Annotations in SQL](#type-annotations-in-sql)
  - [Schema-Based Inference](#schema-based-inference)
- [CLI Commands](#cli-commands)
  - [Rewrite](#rewrite)
  - [Strip](#strip)
  - [Check](#check)
- [Configuration](#configuration)
- [Magic Codec](#magic-codec)
- [API Reference](#api-reference)
  - [SQL Type](#sql-type)
  - [Type Mappings](#type-mappings)
- [Examples](#examples)
- [Contributing](#contributing)

## Installation

### Using pip

```bash
pip install sqlty
```

### Using uv (recommended for development)

```bash
uv add sqlty
```

### From source

```bash
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty
uv sync
```

### Requirements

- Python 3.12+
- sqlglot (for SQL parsing and type inference)

## Quick Start

1. **Write SQL queries with `--sql\n` prefix (newline separates prefix from query):**

```python
# queries.py
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
```

2. **Rewrite to add type annotations:**

```bash
sqlty rewrite queries.py
```

This transforms the file to:

```python
# queries.py
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

3. **Check for issues:**

```bash
sqlty check .
```

4. **Strip annotations (reverse):**

```bash
sqlty strip queries.py
```

## Basic Usage

### Writing SQL Queries

Write SQL queries as plain strings with the `--sql` prefix. Use PostgreSQL-style type casts (`::TYPE`) to declare column types:

```python
# Simple query with explicit casts (--sql\n prefix)
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"

# Multi-line queries (triple-quoted — newline is natural)
query = """--sql
SELECT id::INTEGER, name::TEXT, email::TEXT
FROM users
WHERE active = true
"""
```

Run `sqlty rewrite .` to add type annotations:

```python
from sqlty import SQL

query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
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
| `::TIME` | `datetime.time` |
| `::INTERVAL` | `datetime.timedelta` |
| `::BYTEA` | `bytes` |
| `::UUID` | `uuid.UUID` |
| `::JSON` / `::JSONB` | `Any` |

Example:

```python
q = """--sql
SELECT
    id::INTEGER,
    name::TEXT,
    is_active::BOOLEAN,
    created_at::TIMESTAMP,
    birth_date::DATE,
    duration::INTERVAL
FROM users
"""

# After rewrite:
# q = SQL[tuple[int, str, bool, datetime.datetime, datetime.date, datetime.timedelta]](...)
```

### Schema-Based Inference

Provide a schema file for type inference without explicit casts:

```toml
# pyproject.toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```python
# Before: no casts needed with schema
q = "--sql\nSELECT id, name, email FROM users"

# After rewrite: types inferred from schema
q = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

## CLI Commands

```
sqlty <COMMAND> [OPTIONS] [PATH...]

Commands:
  rewrite     Rewrite source files: "--sql\n..." -> SQL[T]("--sql\n...")
  strip       Strip SQL[T](...) wrappers back to plain strings
  check       Lint: validate type annotations, detect issues
  version     Print version
```

### Rewrite

Transform `"--sql\n..."` strings into `SQL[T]("--sql\n...")` calls with inferred types:

```bash
# Rewrite all Python files in current directory
sqlty rewrite .

# Check what would be rewritten (dry run)
sqlty rewrite --check .

# Show diff without applying
sqlty rewrite --diff .

# With schema for enhanced inference
sqlty rewrite --schema schema.sql .
```

### Strip

Reverse of rewrite — remove `SQL[T](...)` wrappers back to plain `"--sql\n..."` strings:

```bash
# Strip all wrappers
sqlty strip .

# Check if any files contain wrappers
sqlty strip --check .

# Show diff
sqlty strip --diff .
```

### Check

Lint SQL type annotations and detect issues:

```bash
# Run all lint checks
sqlty check .

# With schema for column validation
sqlty check --schema schema.sql .
```

### Common Options

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
return-format = "tuple"     # tuple, typeddict, namedtuple, pydantic

[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

CLI flags override config file values. See [PLAN.md](../PLAN.md) for the full configuration reference.

### Lint Rules

| Rule | Default | Description |
|------|---------|-------------|
| `column-count-mismatch` | error | Annotation has different field count than query |
| `column-type-mismatch` | error | Annotation field type differs from inferred type |
| `column-name-mismatch` | warn | Annotation field name differs from column name |
| `select-star` | warn | `SELECT *` used (fixable: expand to explicit columns) |
| `select-star-no-schema` | error | `SELECT *` used but no schema available |
| `stale-rewrite` | warn | `--sql` strings not wrapped in `SQL[T](...)` |
| `stale-type` | warn | Type annotation doesn't match current inference |

## Magic Codec

SQLTy includes an optional codec that transforms `--sql` strings at import time:

```python
# coding: sqlty

# Strings with --sql prefix are automatically transformed at import
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# Becomes: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

See [magic codec documentation](magic-codec.md) for setup and details.

## API Reference

### SQL Type

```python
from sqlty import SQL
```

Generic type representing typed SQL query strings:

```python
class SQL[T](str):
    pass
```

Used as return type annotation from `sqlty rewrite`:

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Type Mappings

See [Type Annotations in SQL](#type-annotations-in-sql) for the complete SQL-to-Python type mapping table.

## Examples

- **Quick Start**: Write `--sql` queries, run `sqlty rewrite .`
- **Schema inference**: Configure `schema = "schema.sql"` in `[tool.sqlty]`
- **Magic codec**: Use `# coding: sqlty` for import-time transformation

See [Examples Guide](examples.md) for detailed working examples.

## Contributing

### Development Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/jirikuncar/sqlty.git
   cd sqlty
   ```

2. Install dependencies:
   ```bash
   uv sync --all-groups
   ```

3. Run tests:
   ```bash
   make test
   ```

4. Run pre-commit:
   ```bash
   make pre-commit
   ```

### Code Style

This project uses:
- `ruff` for linting and formatting
- `mypy` for type checking
- `pytest` for testing (including markdown-based mdtests)

See [DEVELOPMENT.md](../DEVELOPMENT.md) for full development guide.

## License

MIT License - see LICENSE file for details.

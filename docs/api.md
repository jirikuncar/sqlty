# API Reference

## CLI

```
sqlty <COMMAND> [OPTIONS] [PATH...]
```

### `sqlty rewrite`

Rewrite `"--sql\n..."` strings into `SQL[T]("--sql\n...")` calls with inferred types.

```bash
sqlty rewrite [OPTIONS] [PATH...]
```

| Option | Description |
|--------|-------------|
| `--check` | Exit 1 if files need rewriting (don't modify) |
| `--diff` | Show diff without applying |
| `--dialect <DIALECT>` | SQL dialect: `postgres` (default), `sqlite`, `mysql` |
| `--schema <PATH>` | Schema `.sql` file for enhanced inference |
| `-v, --verbose` | Increase verbosity |
| `-q, --quiet` | Suppress non-error output |

### `sqlty strip`

Strip `SQL[T](...)` wrappers back to plain `"--sql\n..."` strings.

```bash
sqlty strip [OPTIONS] [PATH...]
```

| Option | Description |
|--------|-------------|
| `--check` | Exit 1 if files contain wrappers |
| `--diff` | Show diff without applying |
| `-v, --verbose` | Increase verbosity |
| `-q, --quiet` | Suppress non-error output |

### `sqlty check`

Lint SQL type annotations and detect issues.

```bash
sqlty check [OPTIONS] [PATH...]
```

| Option | Description |
|--------|-------------|
| `--dialect <DIALECT>` | SQL dialect: `postgres` (default), `sqlite`, `mysql` |
| `--schema <PATH>` | Schema `.sql` file |
| `-v, --verbose` | Increase verbosity |
| `-q, --quiet` | Suppress non-error output |

**Exit codes:** `0` = no errors, `1` = errors found.

### `sqlty version`

Print version and exit.

---

## Core Types

### `SQL[T]`

```python
from sqlty import SQL
```

Generic `str` subclass parameterized by the inferred return type:

```python
class SQL[T](str):
    pass
```

Works everywhere a string works. Created by `sqlty rewrite`:

```python
query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

---

## Configuration

### `pyproject.toml`

```toml
[tool.sqlty]
dialect = "postgres"           # SQL dialect (postgres, mysql, sqlite)
schema = "schema.sql"          # Path to schema file
format-command = "ruff format --stdin-filename {filename} -"

[tool.sqlty.rules]             # Per-rule severity
column-count-mismatch = "error"
column-type-mismatch = "error"
column-name-mismatch = "warn"
select-star = "warn"
select-star-no-schema = "error"
stale-rewrite = "warn"
stale-type = "warn"
inline-sql-comment = "warn"
fstring-skipped = "warn"
tstring-skipped = "warn"
```

CLI flags override config file values.

---

## Type Mappings

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

---

## Lint Rules

| Rule | Default | Fixable | Description |
|------|---------|---------|-------------|
| `column-count-mismatch` | error | no | Annotation field count differs from query columns |
| `column-type-mismatch` | error | no | Annotation field type differs from inferred type |
| `column-name-mismatch` | warn | no | Annotation field name differs from column name |
| `select-star` | warn | yes | `SELECT *` — expand to explicit columns |
| `select-star-no-schema` | error | no | `SELECT *` with no schema available |
| `stale-rewrite` | warn | yes | `--sql` strings not wrapped in `SQL[T](...)` |
| `stale-type` | warn | yes | Type annotation doesn't match current inference |
| `inline-sql-comment` | warn | no | `"--sql SELECT ..."` makes query a SQL comment |
| `fstring-skipped` | warn | no | f-string with `--sql` skipped (cannot be statically rewritten) |
| `tstring-skipped` | warn | no | t-string with `--sql` skipped (cannot be statically rewritten) |

Severity values: `"error"`, `"warn"`, `"ignore"`.

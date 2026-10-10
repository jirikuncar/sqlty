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
| `--untyped <MODE>` | Row-returning queries of unknown row type: `skip` (default, leave unwrapped) or `sentinel` (`SQL[UNTYPED](...)`); statements without rows stay unwrapped |
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

### `row_form(query[, default])`

```python
from sqlty import row_form
```

Returns the row type `T` of a query built as `SQL[T](...)`, typed as `TypeForm[T]`:

```python
row_form(query)        # tuple[int, str]
row_form(SQL("..."), None)  # None: no runtime type
```

Without `default`, a query with no runtime type raises `TypeError`. This covers plain strings, `SQL("...")`, annotation-only `q: SQL[T] = "..."` and stripped sources.

## psycopg3 Driver

```python
from sqlty.drivers.psycopg import execute, row_factory, scalar
```

### `row_factory(T)`

Returns the psycopg `BaseRowFactory[T]` for a row type:

| `T` | Row factory |
|-----|-------------|
| `tuple[...]` | `tuple_row` |
| `TypedDict`, `dict[...]` | `dict_row` |
| any other class (dataclass, `NamedTuple`, Pydantic model) | `class_row(T)` |

### `execute(conn, query, params=None)`

Executes `query: SQL[T]` and returns a `Cursor[T]` whose row factory is `row_factory(row_form(query))`, or `tuple_row` when the query has no runtime type:

```python
users = execute(conn, SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")).fetchall()  # list[User]
```

### `scalar(conn, query, [form,] params=None)`

Returns the single column of the first row. The value is typed as `T` from `query: SQL[tuple[T]]`, or as an explicit `form: TypeForm[T]` for plain `LiteralString` queries and narrowing. Unlike `type[T]`, a `TypeForm` accepts unions, `Literal` and other type expressions:

```python
email = scalar(conn, SQL[tuple[str | None]]("--sql\nSELECT email::TEXT FROM users WHERE id = %s"), (1,))  # str | None
role = scalar(conn, "--sql\nSELECT role::TEXT FROM users LIMIT 1", Literal["admin", "user"])
n = scalar(conn, "--sql\nSELECT max(id)::INTEGER FROM users", int | None)
```

`form` is not validated at runtime. A query yielding no rows raises `LookupError`, so `None` always means SQL `NULL`; a row with more than one column raises `ValueError`.

---

## Configuration

### `pyproject.toml`

```toml
[tool.sqlty]
dialect = "postgres"           # SQL dialect (postgres, mysql, sqlite)
schema = "schema.sql"          # Path to schema file
format-command = "ruff format --stdin-filename {filename} -"
untyped = "skip"               # "sentinel": SQL[UNTYPED] for row-returning queries of unknown type
                               # and for the catch-all overload of generated .pyi stubs

[tool.sqlty.rules]             # Per-rule severity
column-count-mismatch = "error"
column-type-mismatch = "error"
column-name-mismatch = "warn"
typeddict-key-mismatch = "error"
typeddict-inheritance = "error"
select-star = "warn"
select-star-no-schema = "error"
stale-rewrite = "warn"
stale-type = "warn"
inline-sql-comment = "warn"
fstring-skipped = "warn"
tstring-skipped = "warn"
untyped-query = "warn"
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
| `typeddict-key-mismatch` | error | no | Query columns don't fit a closed / `extra_items` TypedDict's keys |
| `typeddict-inheritance` | error | no | TypedDict subclass reopens or adds keys a closed / `extra_items` base forbids |
| `select-star` | warn | yes | `SELECT *` — expand to explicit columns |
| `select-star-no-schema` | error | no | `SELECT *` with no schema available |
| `stale-rewrite` | warn | yes | `--sql` strings not wrapped in `SQL[T](...)` |
| `stale-type` | warn | yes | Type annotation doesn't match current inference |
| `inline-sql-comment` | warn | no | `"--sql SELECT ..."` makes query a SQL comment |
| `fstring-skipped` | warn | no | f-string with `--sql` skipped (cannot be statically rewritten) |
| `tstring-skipped` | warn | no | t-string with `--sql` skipped (cannot be statically rewritten) |
| `untyped-query` | warn | no | `SQL[UNTYPED]` query has no inferable row type |

Severity values: `"error"`, `"warn"`, `"ignore"`.

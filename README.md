# SQLTy

[![PyPI](https://img.shields.io/pypi/v/sqlty.svg)](https://pypi.org/project/sqlty/)
[![Python 3.14+](https://img.shields.io/badge/python-3.14+-blue.svg)](https://www.python.org/downloads/)
[![Tests](https://github.com/jirikuncar/sqlty/actions/workflows/tests.yml/badge.svg)](https://github.com/jirikuncar/sqlty/actions/workflows/tests.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Your SQL already knows its types. SQLTy tells Python.

Type-safe raw SQL without an ORM or a database connection. sqlty reads row and
parameter types from your SQL and schema, and checks them on every commit, so a
migration that changes a column is caught in review, not in production.

<p align="center">
  <img src="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/demo.gif" alt="A terminal session: sqlty rewrite adds the row type to a query, a migration makes email nullable, sqlty check --fix updates the type and mypy reports the line that would crash" width="720">
  <br>
  <em>A migration makes <code>email</code> nullable. sqlty and mypy find the code that would crash.</em>
  (<a href="https://github.com/jirikuncar/sqlty/blob/main/docs/assets/demo.tape">view source</a>)
</p>

**Documentation: [jirikuncar.github.io/sqlty](https://jirikuncar.github.io/sqlty/)**

**Fits if** you write SQL as string literals on Python 3.14+, against
PostgreSQL, SQLite or MySQL, with a schema dump or SQL migrations in the
repository. Rows are typed end to end with psycopg 3; with other drivers sqlty
checks the query and its parameters, and you annotate what the cursor returns
([Drivers](https://jirikuncar.github.io/sqlty/guide/drivers-and-orms/)).

> **Beta.** APIs and configuration may change before 1.0. Feedback welcome in
> [GitHub Issues](https://github.com/jirikuncar/sqlty/issues).

## How it works

Drivers return rows of `Any`, so nothing ties a query's columns to the code
that reads them. Start a query with `--sql` and a newline, and
`sqlty rewrite .` adds the row type inferred from your `CREATE TABLE`
statements:

```toml
# pyproject.toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(255) NOT NULL);
```

```python
query = "--sql\nSELECT id, name, email FROM users"
# rewritten: query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

`SQL[T]` is a `str` subclass, so the query still runs anywhere a string does,
and mypy, pyright and ty see `T`. Without a schema, casts such as `id::INTEGER`
type the columns; a query with neither stays as it is. With a space instead of
the newline, `"--sql SELECT ..."` is a SQL comment, and `sqlty check` warns.

## What it catches

### A migration changes a column

```toml
[tool.sqlty]
schema = "schema.sql"
```

A migration dropped `NOT NULL` from `email`.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);
```

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int, str]] should be SQL[tuple[int, str | None]]"
query = SQL[tuple[int, str]]("--sql\nSELECT id, email FROM users")
```

`sqlty check --fix .` updates the type, and your type checker finds every
caller that now needs a `None` check. `stale-type` is a warning by default; set
`stale-type = "error"` under `[tool.sqlty.rules]` to fail CI on it
([CI](https://jirikuncar.github.io/sqlty/recipes/ci/)). For rolling deploys,
`sqlty breaking --against origin/main` exits 1 when old code breaks on the new
schema or new code on the old one
([Breaking schema changes](https://jirikuncar.github.io/sqlty/guide/breaking-changes/)).

### A row type that doesn't match its query

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

`TypedDict`, `NamedTuple`, dataclasses and Pydantic models are all checked
([Row types](https://jirikuncar.github.io/sqlty/guide/row-types/)).

### Wrong parameters, misspelled columns

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-column = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);
```

```python
import sqlite3
from sqlty import SQL

conn = sqlite3.connect("app.db")

# error: [param-count-mismatch] "query has 1 parameter, the call passes 2"
conn.execute(SQL[tuple[str | None]]("--sql\nSELECT email FROM users WHERE id = ?"), (1, 2))

# error: [unknown-column] "column `emial` does not exist in `users`"
conn.execute("--sql\nSELECT emial FROM users")
```

`?`, `$1`, `%s` and `:name` placeholders are checked on sqlite3, psycopg and
asyncpg calls ([Parameters](https://jirikuncar.github.io/sqlty/guide/parameters/)).
These are the mistakes coding agents make: every problem is one
`file:line: [rule] message` line and a non-zero exit code they can act on, and
the [sqlty skill](https://jirikuncar.github.io/sqlty/recipes/agents/) teaches
them the conventions.

## Get started

```bash
pip install "sqlty[psycopg]"   # or: uv add sqlty; the extra adds the psycopg helpers
printf '%s\n' 'q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"' > queries.py
sqlty rewrite -v .             # q = SQL[tuple[int, str]]("--sql\n...")
sqlty check .                  # report drift and mismatches
sqlty strip .                  # undo: back to plain strings
```

`rewrite` and `strip` only touch the wrapper, never the SQL, so you can adopt
sqlty one module at a time and back out. New project:
[Quickstart](https://jirikuncar.github.io/sqlty/quickstart/). Existing codebase:
[Migration](https://jirikuncar.github.io/sqlty/recipes/migration/), starting
with `pg_dump --schema-only app > schema.sql`.

Not a fit for ORMs, SQL built at runtime, or runtime validation;
[Why SQLTy?](https://jirikuncar.github.io/sqlty/motivation/) compares it with
sqlc, sqlx, PgTyped and others.

## About

The engine is Rust ([sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs)
via [PyO3](https://pyo3.rs)), with wheels for Linux, macOS and Windows. SQLTy is
developed with AI coding agents, mainly Claude Code; every change is reviewed by
a maintainer (see the
[AI-assisted contributions](https://github.com/jirikuncar/sqlty/blob/main/CONTRIBUTING.md#ai-assisted-contributions)
policy). [Contributing](https://github.com/jirikuncar/sqlty/blob/main/CONTRIBUTING.md) ·
[Credits](https://jirikuncar.github.io/sqlty/credits/) ·
MIT [License](https://github.com/jirikuncar/sqlty/blob/main/LICENSE).

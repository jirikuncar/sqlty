# SQLTy

[![PyPI](https://img.shields.io/pypi/v/sqlty.svg)](https://pypi.org/project/sqlty/)
[![Python 3.14+](https://img.shields.io/badge/python-3.14+-blue.svg)](https://www.python.org/downloads/)
[![Tests](https://github.com/jirikuncar/sqlty/actions/workflows/tests.yml/badge.svg)](https://github.com/jirikuncar/sqlty/actions/workflows/tests.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Your SQL already knows its types. SQLTy tells Python.

Type-safe raw SQL, no ORM and no database connection: a migration that changes
a column is caught in CI, not in production.

<p align="center">
  <img src="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/demo.gif" alt="A terminal session: sqlty rewrite adds the row type to a query, a migration makes email nullable, sqlty check --fix updates the type and mypy reports the line that would crash" width="720">
  <br>
  <em>A migration makes <code>email</code> nullable. sqlty and mypy find the code that would crash.</em>
  (<a href="https://github.com/jirikuncar/sqlty/blob/main/docs/assets/demo.tape">view source</a>)
</p>

**Documentation: [jirikuncar.github.io/sqlty](https://jirikuncar.github.io/sqlty/)**

> **Beta.** APIs and configuration may change before 1.0. Feedback welcome in
> [GitHub Issues](https://github.com/jirikuncar/sqlty/issues).

## How it works

Drivers return rows of `Any`, so nothing ties a query's columns to the code
that reads them. Mark the query with `--sql\n` and `sqlty rewrite .` adds the
row type inferred from your `CREATE TABLE` statements (or from casts):

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

`SQL[T]` is a `str` subclass: the query still runs anywhere a string does, and
mypy, pyright and ty now know its row type. The `# rewritten:` comments are
checked by the test suite.

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

`sqlty check --fix .` updates the type and your type checker finds every caller
that now needs a `None` check.

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

## Get started

```bash
pip install sqlty    # or: uv add sqlty
sqlty rewrite .      # add SQL[T] to --sql strings
sqlty check .        # report drift and mismatches
sqlty strip .        # undo: back to plain strings
```

Adoption is reversible and incremental: `rewrite` and `strip` only touch the
wrapper, never the SQL. Next: [Quickstart](https://jirikuncar.github.io/sqlty/quickstart/),
[CI](https://jirikuncar.github.io/sqlty/recipes/ci/),
[breaking schema changes](https://jirikuncar.github.io/sqlty/guide/breaking-changes/),
[psycopg](https://jirikuncar.github.io/sqlty/guide/psycopg/),
[AI agents](https://jirikuncar.github.io/sqlty/recipes/agents/).

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

---
title: "How to execute raw SQL in Python, type-safe"
description: "Performing raw SQL queries in Python with sqlite3, psycopg, asyncpg, aiosqlite, SQLAlchemy text() and Django, with typed rows and checked parameters."
---

# Execute raw SQL in Python

Every Python database driver runs raw SQL the same way: a query string, its
parameters, and rows back. The driver can't tell your type checker what those
rows hold, so they arrive as `Any` or `tuple[Any, ...]`, and a typo in a column
name shows up only when the query runs.

sqlty reads the query and your schema, and gives each query a row type. This page
shows how to perform raw SQL queries with each common driver so that the rows are
typed and the SQL checked against the schema, without a database.

All examples use this schema:

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT
);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## The three steps

1. Write the query as a string that starts with `--sql` on its own line.
2. `sqlty rewrite .` wraps it in `SQL[T](...)`, `T` being the row type inferred
   from the query and the schema. `SQL[T]` is a `str`, so the driver takes it as is.
3. `sqlty check .` reports queries that don't match the schema, rows annotated
   with the wrong type, and calls that pass the wrong parameters.

```python
query = "--sql\nSELECT id, name, email FROM users WHERE id = %s"
# rewritten: query = SQL[tuple[int, str, str | None], tuple[int | None]]("--sql\nSELECT id, name, email FROM users WHERE id = %s")
```

See [Writing queries](writing-queries.md) for the marker and
[Query parameters](parameters.md) for the parameter types.

## Pass values as parameters

Never build a query with an f-string, `%` or `+`. A value merged into the SQL
text can change what the query does (SQL injection), and sqlty skips f-strings
because their SQL isn't known until run time. Every driver takes the values
separately, in its own placeholder style:

| Driver | Placeholders | Parameters | Calls checked |
|--------|--------------|------------|---------------|
| `sqlite3`, aiosqlite | `?`, `:name` | a tuple, or a dict for `:name` | yes |
| psycopg 3 | `%s`, `%(name)s` | a tuple, or a dict for `%(name)s` | yes |
| psycopg2 | `%s`, `%(name)s` | a tuple, or a dict for `%(name)s` | no |
| asyncpg | `$1`, `$2` | positional arguments after the query | yes |
| SQLAlchemy `text()` | `:name` | a dict | no |
| Django `connection.cursor()` | `%s` | a list or tuple | no |

A single value still goes in a sequence: `(user_id,)`, not `(user_id)`.
`sqlty check` compares the parameters a call passes with the query where the
driver is checked: see [Driver calls](parameters.md#driver-calls). Elsewhere it
checks the SQL and the row type.

## sqlite3

[`Connection.execute`](https://docs.python.org/3/library/sqlite3.html#sqlite3.Connection.execute)
creates a cursor and runs the query on it. sqlty checks the parameters of the call:

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
import sqlite3

conn = sqlite3.connect("app.db")
user = conn.execute("SELECT id, name FROM users WHERE id = ?", (1,)).fetchone()

# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
conn.execute("UPDATE users SET name = ? WHERE id = ?", ("Ada",))
```

## psycopg 3

`conn.execute(query, params)` is the psycopg 3 shortcut for a cursor's
`execute`. With [`sqlty.drivers.psycopg`](psycopg.md), the rows come back as the
query's `T`, built by the matching row factory:

```python
from typing import NamedTuple

import psycopg
from sqlty import SQL
from sqlty.drivers.psycopg import execute


class User(NamedTuple):
    id: int
    name: str


# error:
by_id = SQL[User]("--sql\nSELECT id, name FROM users WHERE id = %s")


def find(conn: psycopg.Connection) -> User | None:
    return execute(conn, by_id, (1,)).fetchone()
```

A query that doesn't match its row type is reported where it is written:

```python
from typing import NamedTuple

from sqlty import SQL


class User(NamedTuple):
    id: int
    name: str


# error: [column-count-mismatch] "User has 2 fields but query returns 3 columns"
by_id = SQL[User]("--sql\nSELECT id, name, email FROM users WHERE id = %s")
```

## asyncpg

asyncpg numbers its placeholders `$1`, `$2` and takes the values as positional
arguments of `fetch`, `fetchrow`, `fetchval` and `execute`:

```python
import asyncpg


async def rename(user_id: int, name: str) -> None:
    conn = await asyncpg.connect()
    row = await conn.fetchrow("SELECT id, name FROM users WHERE id = $1", user_id)
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await conn.execute("UPDATE users SET name = $1 WHERE id = $2", name)
```

## aiosqlite

aiosqlite wraps `sqlite3` for `asyncio`. A connection passed to a function is
checked when its parameter is annotated:

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
import aiosqlite


async def rename(db: aiosqlite.Connection, name: str) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute("UPDATE users SET name = ? WHERE id = ?", (name,))
```

## SQLAlchemy `text()`

[`text()`](https://docs.sqlalchemy.org/en/20/core/sqlelement.html#sqlalchemy.sql.expression.text)
runs a raw SQL string through SQLAlchemy, with `:name` placeholders and a dict
of values. sqlty reads `:name` with the `postgres` and `sqlite` dialects:

```python
from typing import NamedTuple

from sqlalchemy import Connection, text
from sqlty import SQL


class User(NamedTuple):
    id: int
    name: str


# error:
by_name = SQL[User]("--sql\nSELECT id, name FROM users WHERE name = :name")


def find(conn: Connection, name: str) -> list[User]:
    return [User(*row) for row in conn.execute(text(by_name), {"name": name})]
```

ORMs may convert some values before your code sees them, such as SQLite dates to
`datetime.date`. [Drivers and ORMs](drivers-and-orms.md) lists those and what to
annotate.

## Django

Django runs raw SQL through `connection.cursor()`, with `%s` placeholders for
every database backend. Mark the query and set `dialect` to your database (on
MySQL, write the marker as `-- sql`, see [Writing queries](writing-queries.md)):

```python
from django.db import connection


def names() -> list[tuple[str]]:
    with connection.cursor() as cursor:
        # error: [unknown-table] "table `accounts` is not defined in the schema"
        cursor.execute("--sql\nSELECT name FROM accounts")
        return cursor.fetchall()
```

## Check every query in CI

`sqlty check .` exits 1 when a rule of severity `error` fires, so it fits a CI job or a pre-commit
hook: see [CI and pre-commit](../recipes/ci.md). Your type checker (mypy, pyright
or ty) then sees the row type of each `SQL[T]` query.

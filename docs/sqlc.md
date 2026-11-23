---
title: "SQLTy vs sqlc: typed SQL for Python without code generation"
description: "How SQLTy compares with sqlc and its Python plugins: generated query functions from .sql files, or SQL[T] checked inline against your schema files."
---

# SQLTy vs sqlc

[sqlc](https://sqlc.dev) reads your schema and annotated `.sql` query files and
generates typed code that runs them. Python gets that code from a plugin:
[sqlc-gen-python](https://github.com/sqlc-dev/sqlc-gen-python) or
[sqlc-gen-better-python](https://github.com/rayakame/sqlc-gen-better-python).
SQLTy generates nothing: the query stays a string in your code, or a named query
in the same `.sql` files, and `sqlty check` types it against the schema.

## The same query in both

sqlc, with `schema.sql`, `query.sql` and a `codegen` entry for the Python plugin
in `sqlc.yaml`:

```sql
-- name: GetUser :one
SELECT id, name, email FROM users WHERE id = $1;
```

`sqlc generate` writes a `models.py` with a `User` dataclass, and code that runs
the query and builds a `User` from the row: a `Querier` class with a `get_user`
method (sqlc-gen-python) or a `get_user` function (sqlc-gen-better-python).

SQLTy, with `schema = "schema.sql"` under `[tool.sqlty]`:

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
```

```python
from dataclasses import dataclass

from sqlty import SQL


@dataclass
class User:
    id: int
    name: str
    email: str | None


GET_USER = SQL[User]("--sql\nSELECT id, name, email FROM users WHERE id = %s")
```

`sqlty check .` compares `User` with the columns the query returns. You run
`GET_USER` with your driver, and the [psycopg helpers](guide/psycopg.md) return
`User` rows typed end to end.

## Side by side

| | sqlc 1.31 with a Python plugin | SQLTy |
|-|--------------------------------|-------|
| Written in | Go; plugins are WebAssembly modules or processes | Rust, installed from PyPI |
| Where queries live | Annotated `.sql` files (`-- name: GetUser :one`) | String literals in Python, or the same annotated `.sql` files |
| Output | Generated Python modules, regenerated on every change | No generated code: `SQL[T]` annotations that `sqlty rewrite` writes into your source |
| Row type | A generated dataclass, or a Pydantic, attrs or msgspec model per table or query | Your own tuple, `TypedDict`, `NamedTuple`, dataclass or Pydantic model, checked against the query |
| Driver | sqlc-gen-python: SQLAlchemy `Connection` or `AsyncConnection`; sqlc-gen-better-python: asyncpg, psycopg, sqlite3, aiosqlite, asyncmy or PyMySQL | Any: `sqlite3`, psycopg, asyncpg, SQLAlchemy `text()`, Django; see [Drivers and ORMs](guide/drivers-and-orms.md) |
| Where types come from | `CREATE TABLE` statements; optionally a PostgreSQL database for harder queries | `CREATE TABLE` statements and casts; never a database |
| Dynamic SQL | One query per annotation; [`sqlc.slice`](https://docs.sqlc.dev/en/latest/reference/macros.html) and `sqlc.narg` for variations | A string built in `if`/`else` branches, checked once per [variant](guide/query-variants.md) (preview) |
| Lint | `sqlc vet`: rules you write in CEL, plus `sqlc/db-prepare` | [Built-in rules](reference/rules.md) with fixes, such as fan-out aggregates and `SELECT *` |
| Breaking changes | `sqlc verify`, against queries pushed to sqlc's hosted service | [`sqlty breaking`](guide/breaking-changes.md), against a git revision or a snapshot, locally |
| Editor | Generated code is plain Python | Language server, VS Code extension, [playground](playground.md) |

## Generated code or checked strings

sqlc's model works well when every query is fixed and its callers only need a
function to call. It costs a build step, a generated layer to review, and one
calling convention per query. SQLTy keeps the code you wrote: the query is a
`str` subclass, so any driver, helper or ORM `text()` call takes it, and mypy,
Pyright and ty see its row type.

The official sqlc-gen-python plugin types PostgreSQL columns only: with another
engine, its columns come out as `Any`
([source](https://github.com/sqlc-dev/sqlc-gen-python/blob/main/internal/gen.go)).
SQLTy types PostgreSQL, MySQL and SQLite queries, and DuckDB on a best-effort basis.

## Keep your sqlc queries

SQLTy reads sqlc's query files, so a project shared with Go or another sqlc
target can check the same files from its Python side:

```toml
[tool.sqlty]
schema = "schema.sql"
queries = ["query.sql"]
query-format = "sqlc"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
```

```sql
-- name: GetUser :one
-- error: [unknown-column] "column `emial` does not exist in `users`; did you mean `email`?"
SELECT id, name, emial FROM users WHERE id = $1;
```

`@name`, `sqlc.arg(name)` and `sqlc.narg(name)` are read as parameters, and
`sqlc.embed(t)` as the columns of `t`. See [Queries in `.sql` files](guide/sql-files.md).

## Sources

- [sqlc: generating code](https://docs.sqlc.dev/en/latest/howto/generate.html),
  [database and language support](https://docs.sqlc.dev/en/latest/reference/language-support.html),
  [`vet`](https://docs.sqlc.dev/en/latest/howto/vet.html) and
  [`verify`](https://docs.sqlc.dev/en/latest/howto/verify.html)
- [Migrating to sqlc-gen-python](https://docs.sqlc.dev/en/latest/guides/migrating-to-sqlc-gen-python.html)
- [sqlc-gen-better-python](https://github.com/rayakame/sqlc-gen-better-python): drivers and model types

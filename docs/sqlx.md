---
title: "SQLTy vs SQLx: compile-time checked SQL for Python"
description: "How SQLTy compares with Rust's SQLx: query macros checked against a live database or an offline cache, and SQL[T] checked statically against your schema files."
---

# SQLTy vs SQLx

[SQLx](https://github.com/transact-rs/sqlx) checks raw SQL in Rust at compile
time: the `query!` macros send each query to a database and turn its answer into
types. SQLTy gives Python the same guarantee, but reads the types from your
schema files instead of asking a database. Both keep the SQL as you wrote it.

## The same query in both

SQLx, with `DATABASE_URL` set at build time:

```rust
struct User {
    id: i32,
    name: String,
    email: Option<String>,
}

let user = sqlx::query_as!(User, "SELECT id, name, email FROM users WHERE id = $1", id)
    .fetch_one(&pool)
    .await?;
```

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

`sqlty check .` compares `User` with the columns the query returns, as
`query_as!` compares the fields of your struct. `sqlty rewrite .` writes `T` for a
query that has none yet, as `query!` builds an anonymous record.

## Side by side

| | SQLx 0.9 | SQLTy |
|-|----------|-------|
| Language | Rust | Python 3.14+ |
| When it checks | `cargo build`, in the macro expansion | `sqlty check`, the language server, pre-commit or CI |
| Where types come from | A live database at `DATABASE_URL`, or the `.sqlx` cache that `cargo sqlx prepare` writes from one | `CREATE TABLE` statements in a schema dump or migrations, and casts in the query |
| Row type | `query!`: an anonymous record; `query_as!`: your struct | `SQL[T]`: a tuple, `TypedDict`, `NamedTuple`, dataclass or Pydantic model, written by `sqlty rewrite` |
| Seen by | The Rust compiler | mypy, Pyright and ty |
| Parameters | Bind types checked against the database | Placeholder types inferred from where they sit, call arguments checked |
| Nullability | From `NOT NULL`; PostgreSQL expressions assumed nullable; forced with `"id!"` or `"id?"` aliases | From `NOT NULL`, keys, joins, `WHERE ... IS NOT NULL` and expressions |
| Type overrides | `sqlx.toml`: `[macros.type-overrides]`, `[macros.table-overrides]` | `[tool.sqlty.types]`, by SQL type or `table.column` |
| Configuration | `sqlx.toml` | `[tool.sqlty]` in `pyproject.toml`, or `sqlty.toml` |
| Databases | PostgreSQL, MySQL, SQLite | PostgreSQL, MySQL, SQLite; DuckDB best effort |
| Migrations | `sqlx migrate` runs them | Read as the schema; `sqlty breaking` checks the old and new code of a deploy against them |
| Query text | A string literal | A string literal |

## The database at check time

SQLx needs a database with the current schema whenever a query changes. In CI it
reads the committed `.sqlx` directory instead, and `cargo sqlx prepare --check`
fails when that cache is stale. SQLTy never connects: the schema file in the
repository is the source of truth, so CI, editors and the
[playground](playground.md) check queries with nothing running. The price is
that SQLTy types the schema as the files declare it, not as a server would
resolve it; see [Schema inference](guide/schema-inference.md).

## Configuration

SQLx 0.9 reads crate-wide settings from one file:

`sqlx.toml`:

```toml
[common]
database-url-var = "APP_DATABASE_URL"

[macros.type-overrides]
'UUID' = "crate::types::MyUuid"
```

SQLTy reads them from `[tool.sqlty]`:

```toml
[tool.sqlty]
dialect = "postgres"
schema = ["schema.sql", "migrations/"]

[tool.sqlty.types]
"users.id" = "app.types.UserId"
```

See [Configuration](reference/configuration.md) for every key.

## Beyond what SQLx checks

- [`sqlty breaking`](guide/breaking-changes.md) checks old code against the new
  schema and new code against the old one, the two mixes of a rolling deploy.
- [Lint rules](reference/rules.md) report fan-out aggregates, joins on the wrong
  key and `SELECT *`.
- [`sqlty format`](guide/formatting.md) formats the SQL inside Python strings.

## Sources

- [SQLx 0.9.0 release notes](https://github.com/transact-rs/sqlx/discussions/4271)
- [`query!`](https://docs.rs/sqlx/latest/sqlx/macro.query.html) and
  [`query_as!`](https://docs.rs/sqlx/latest/sqlx/macro.query_as.html) on docs.rs
- [`sqlx.toml` reference](https://docs.rs/sqlx/latest/sqlx/_config/_reference/index.html)

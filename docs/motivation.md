---
description: Why raw SQL in Python is untyped, how sqlty types it without a database or an ORM, and how it compares with aiosql, PugSQL, sqlc, asyncpg_typed and other prior art.
---

# Why SQLTy?

## The problem

Raw SQL in Python is untyped. The driver returns rows of `Any`, and nothing
connects the columns a query selects to the code that reads them:

```python
row = cur.execute("SELECT id, name, created_at FROM users WHERE id = %s", (user_id,)).fetchone()
row[2].isoformat()  # is column 2 a datetime? Only the SQL knows.
```

When a column is renamed, retyped or reordered, every query and every
consumer has to be found by hand. Mistakes surface at runtime.

## What sqlty does

sqlty reads the SQL, infers the row type from casts or a schema, and puts it
next to the query as `SQL[T]`, a `str` subclass:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# rewritten: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The query stays plain SQL that you can read, copy into a console and run.
Type checkers see `T`; `sqlty check` keeps `T` and the SQL in sync, and
validates your own row types against the columns.

## When to use something else

sqlty fits raw SQL written as string literals, with casts or a schema dump
in the repository; [Use cases](use-cases.md) lists the typical situations.
Use something else for:

- **An ORM** (SQLAlchemy, Django): use its typing support.
- **Dynamic SQL** built at runtime: sqlty only reads string literals.
- **Runtime validation**: sqlty is static analysis. Use Pydantic to validate
  rows as they arrive.

## How it compares

| Approach | Trade-off |
|----------|-----------|
| ORM types | Tied to the ORM and its query API |
| Runtime validation (Pydantic) | Catches mismatches only when the code runs |
| Hand-written annotations | Drift from the SQL silently |
| sqlty | Needs casts or a schema; queries must be literals |

### asyncpg_typed

[asyncpg_typed](https://github.com/hunyadi/asyncpg_typed) (0.1.7) declares the
types of an asyncpg query by hand, `sql(query, args=tuple[int], resultset=Row)`,
and its overloads give `fetch`, `fetchrow` and `fetchval` exact signatures. Each
call prepares the statement and compares the declared types with what the
server reports.

| | sqlty | asyncpg_typed |
|---|-------|---------------|
| Types come from | casts or a schema, written by `sqlty rewrite` | annotations you write |
| Checked | statically, with no database | at run time, on every call, against the live server |
| Databases | PostgreSQL, MySQL, SQLite, DuckDB and other dialects | PostgreSQL through asyncpg |
| Nullability | inferred from `NOT NULL`, joins and expressions | not reported by prepared statements: a `NULL` in a non-optional column raises `NoneTypeError` |
| Parameters | typed from where they sit, checked at call sites; `SQL[R, P]` with [`param-types`](reference/configuration.md#param-types) | declared; the type checker checks `*args` |
| Rows seen by the type checker | `T` through the [psycopg helpers](guide/psycopg.md); asyncpg's `Record` stays `Any` | `list[T]`, `T \| None`, `R1` |
| Row types | `TypedDict`, `NamedTuple`, dataclass, Pydantic, `Protocol` | `tuple`, `NamedTuple` |
| Column names | [`column-name-mismatch`](reference/rules.md#column-name-mismatch) | `NameMismatchError` for a `NamedTuple` |
| Enums | a `Literal` of the values; a `StrEnum` field is accepted without comparing its values | `StrEnum` values compared with `pg_enum` |
| JSON | `Any`, with [JSON shapes](guide/json-shapes.md) | `JsonType`, decoded with `json` or orjson |
| Ranges, geometric types | [`unknown-type`](reference/rules.md#unknown-type), and `str` | `asyncpg.Range[T]`, `asyncpg.Point` and the like |
| t-strings | `{value}` is a parameter | `{1}` is the ordinal of `$1` |
| SQL built at run time | not checked | `unsafe_sql` takes any `str`, checked when it runs |

sqlty lacks:

- Rows typed through asyncpg. asyncpg returns `Record`, so `SQL[T]` stops at the
  call; asyncpg_typed returns `T`.
- asyncpg's types. sqlty follows psycopg, which returns `json` parsed, ranges as
  its own `Range` and geometric types as text.
- A comparison of `StrEnum` values with `CREATE TYPE ... AS ENUM`.
- A check against the live database: a schema dump that drifts from it goes
  unnoticed.

asyncpg_typed lacks:

- Inference: each type is written by hand, and nothing updates it when the query
  changes.
- Static checks: a mismatch surfaces when the query first runs, so tests need a
  PostgreSQL server.
- Nullability: an optional annotation is never reported, and a missing one
  only when a `NULL` arrives.
- Other databases and drivers, and row types other than tuples.
- A free call: each call prepares the statement again, and one with an enum
  column also queries `pg_enum`.

## Prior art

Typed raw SQL is not new. Most tools get column types from a live database;
sqlty works offline, from casts or `CREATE TABLE` statements, and keeps
queries inline as string literals.

| Project | Language | Approach | Live database |
|---------|----------|----------|---------------|
| [sqlx](https://github.com/transact-rs/sqlx) | Rust | `query!` macros check inline SQL at compile time | Yes, or an offline cache |
| [postgresql-typed](https://github.com/dylex/postgresql-typed) | Haskell | Template Haskell prepares each query at compile time | Yes |
| [Doobie](https://github.com/typelevel/doobie) | Scala | `check` validates queries against declared types in tests | Yes |
| [SafeQL](https://github.com/ts-safeql/safeql) | TypeScript | ESLint rule checks inline `sql` tags and fixes their type annotations | Yes |
| [sqlx-ts](https://github.com/JasonShin/sqlx-ts), [mfsqlchecker](https://github.com/MedFlyt/mfsqlchecker) | TypeScript | Check inline queries and their result types | Yes |
| [sqlc](https://github.com/sqlc-dev/sqlc) | Go, Python and others | Generates typed code from schema and `.sql` files | No |
| [SQLDelight](https://github.com/sqldelight/sqldelight) | Kotlin | Generates typed APIs from `.sq` files | No |
| [PgTyped](https://github.com/adelsz/pgtyped) | TypeScript | Generates types for `.sql` files and `sql` tags | Yes |
| [Prisma TypedSQL](https://www.prisma.io/docs/orm/v7/prisma-client/using-raw-sql/typedsql) | TypeScript | Typed functions from `.sql` files | Yes |
| [Cornucopia](https://github.com/cornucopia-rs/cornucopia), [Squirrel](https://github.com/giacomocavalieri/squirrel) | Rust, Gleam | Generate typed functions from `.sql` files | Yes |
| [querky](https://github.com/racinette/querky) | Python | Decorated raw SQL becomes typed functions | Yes |
| [asyncpg_typed](https://github.com/hunyadi/asyncpg_typed) | Python | Declared argument and row types, checked when asyncpg prepares the query | Yes |
| [aiosql](https://github.com/nackjicholson/aiosql), [PugSQL](https://github.com/mcfunley/pugsql), [HugSQL](https://github.com/layerware/hugsql) | Python, Clojure | Load named queries from `.sql` files, untyped | No |
| [sqlglot](https://github.com/tobymao/sqlglot) | Python | `annotate_types` infers expression types from a schema | No |

Query builders such as [Kysely](https://github.com/kysely-org/kysely),
[jOOQ](https://github.com/jOOQ/jOOQ) and SQLAlchemy type results through a
DSL instead of SQL strings. [psycopg](https://www.psycopg.org/psycopg3/docs/advanced/typing.html)
types rows by row factory, which sqlty's [psycopg helpers](guide/psycopg.md)
build on. Linters such as [SQLFluff](https://github.com/sqlfluff/sqlfluff)
and [Squawk](https://github.com/sbdchd/squawk) check style and migrations,
not result types.

### Further reading

Announcements and articles:

- [Introducing sqlc - Compile SQL queries to type-safe Go](https://conroy.org/introducing-sqlc), Kyle Conroy, 2019
- [Taming SQL and ORMs with sqlc](https://encore.dev/blog/go-get-it-001-sqlc), Encore, 2022
- [sqlc on the Technology Radar](https://www.thoughtworks.com/en-au/radar/languages-and-frameworks/sqlc), Thoughtworks, 2022
- [How psycopg row factories help you write safer applications](https://blog.dalibo.com/2022/06/01/psycopg-row-factories.html), Denis Laxalde, 2022
- [Write SQL queries with confidence (TypeScript + Postgres)](https://dev.to/newbie012/write-sql-queries-with-confidence-typescript-postgres-5b61), Eliya Cohen, 2022
- [Raw SQL in Rust with SQLx](https://www.shuttle.dev/blog/2023/10/04/sql-in-rust), Shuttle, 2023
- [TypedSQL: type-safe raw SQL queries in Prisma ORM](https://www.prisma.io/blog/announcing-typedsql-make-your-raw-sql-queries-type-safe-with-prisma-orm), Prisma, 2024
- [Libraries for writing raw SQL safely](https://www.propelauth.com/post/libraries-for-writing-raw-sql-safely), PropelAuth, 2024
- [You don't need an ORM](https://fosdem.org/2026/events/attachments/F9Y7ZY-you-do-not-need-an-orm/slides/266904/2026_-_fo_qvc4ayx.pdf), Giacomo Cavalieri, FOSDEM 2026
- [Python t-strings (PEP 750) vs f-strings](https://www.danilchenko.dev/posts/python-t-strings/), Maksim Danilchenko, 2026
- [Template string queries](https://www.psycopg.org/psycopg3/docs/basic/tstrings.html) in psycopg 3.3
- [sqlc: Python code generation](https://github.com/sqlc-dev/sqlc/pull/923)

Discussions:

- Hacker News: [sqlc](https://news.ycombinator.com/item?id=41478930),
  [PgTyped](https://news.ycombinator.com/item?id=23302549),
  [pg-typesafe](https://news.ycombinator.com/item?id=47050872),
  [SQL-tString](https://news.ycombinator.com/item?id=44004827)
- discuss.python.org: [virality of t-strings](https://discuss.python.org/t/concerned-about-virality-of-t-strings-pep-750/92601),
  [language hints for template strings](https://discuss.python.org/t/language-hints-for-pep-750-template-strings/94311),
  [PEP 718: subscriptable functions](https://discuss.python.org/t/pep-718-subscriptable-functions/28457)
- PEPs: [675 `LiteralString`](https://peps.python.org/pep-0675/),
  [718 subscriptable functions](https://peps.python.org/pep-0718/),
  [750 template strings](https://peps.python.org/pep-0750/)

Other projects: [tysql](https://pypi.org/project/tysql/) (typed query builder
on a mypy fork), [Zapatos](https://github.com/jawj/zapatos),
[Slonik](https://github.com/gajus/slonik),
[Esqueleto](https://github.com/bitemyapp/esqueleto),
[SeaQuery](https://github.com/SeaQL/sea-query),
[Drizzle](https://github.com/drizzle-team/drizzle-orm),
[pgsql-ast-parser](https://github.com/oguimbal/pgsql-ast-parser),
[Yesql](https://github.com/krisajenkins/yesql).

---
description: Answers to common questions about typing raw SQL in Python, passing parameters safely, formatting queries and using sqlty with drivers and ORMs.
---

# FAQ

## How do I type SQL queries in Python?

Start the query string with `--sql`. `sqlty rewrite .` infers the row type from
the casts in the query or from your `CREATE TABLE` statements, and wraps the
string as `SQL[T]`, for example `SQL[tuple[int, str]]`. `SQL[T]` is a
`str` subclass, so the query still runs anywhere a string does, and mypy,
pyright and ty see the row type. `sqlty check .` reports when the SQL and `T` no
longer agree. See the [Quickstart](quickstart.md).

## When should I use sqlty?

When you write raw SQL as string literals and want the type checker to know what
each query returns and takes. sqlty fits a codebase with a schema dump or
migrations in the repository, where a renamed or retyped column should fail CI,
not production. It also fits code that adopts types one query at a time, scripts
and notebooks, and SQL written by AI assistants. See [Use cases](use-cases.md).

## When should I not use sqlty?

When the SQL is not a literal: a query built at run time, by string operations
or a query builder, is not read. When you use an ORM's query API, use the ORM's
typing support instead. sqlty checks statically, so if you need rows validated
as they arrive, use Pydantic. See
[When to use something else](motivation.md#when-to-use-something-else).

## How can I write raw SQL in Python without an ORM?

Keep the SQL as a string literal and pass it to your driver: psycopg, PyMySQL,
`sqlite3` or any other. sqlty adds types around the string without changing it,
so the query runs as written, with no query builder or model layer. With
[psycopg](guide/psycopg.md), `execute(conn, SQL[User](...))` returns a
`Cursor[User]` whose rows are `User` objects. See
[Raw SQL without an ORM](use-cases.md#raw-sql-without-an-orm).

## How do I pass values into a SQL query safely?

Use the driver's placeholders, such as `%s`, `%(name)s`, `$1`, `?` or `:name`,
and pass the values as parameters. Never build the query with f-strings, `%` or
`str.format()`: the driver escapes parameters, but not text pasted into the SQL.
sqlty gives each placeholder a type from where it sits in the query, and
`sqlty check` reports a call that passes the wrong number of values or a value
of the wrong type. See [Query parameters](guide/parameters.md).

## How do I get typed rows instead of tuples of `Any`?

Annotate the query with a row type: a `TypedDict`, `NamedTuple`, dataclass or
Pydantic model, as `SQL[User](...)`. `sqlty check` compares its fields with the
columns the query returns, by count, name and type. With psycopg,
`sqlty.drivers.psycopg.execute` picks the matching row factory, so each row is
built as `User` at runtime. See [Row types](guide/row-types.md).

## Does sqlty need a database connection?

No. sqlty reads column types from casts in the query or from `CREATE TABLE`
statements in a schema file, such as a `pg_dump --schema-only` dump or a
migrations snapshot. It runs offline, in an editor, pre-commit or CI. See
[Schema inference](guide/schema-inference.md).

## Which databases and drivers does sqlty support?

PostgreSQL, MySQL and SQLite are fully supported, with types following psycopg
3, PyMySQL and `sqlite3`. Other dialects that sqlparser-rs reads, such as
DuckDB, BigQuery, Snowflake and ClickHouse, work on a best-effort basis. See
[Drivers and ORMs](guide/drivers-and-orms.md) and the
[Engine](reference/engine.md#dialects).

## Does sqlty work with SQLAlchemy, Django or other ORMs?

Yes, for the raw SQL you run through them, such as SQLAlchemy's `text()` with
`:name` placeholders. sqlty types the value the driver returns, and some ORMs
convert it first, for example SQLite dates to `datetime.date`.
[Drivers and ORMs](guide/drivers-and-orms.md) lists those conversions and what
to annotate.

## Can I keep queries in `.sql` files?

Yes. Point `queries` in `[tool.sqlty]` at your `.sql` files, and `sqlty check`
runs the same rules on the named queries that aiosql, PugSQL and sqlc read. See
[Queries in .sql files](guide/sql-files.md).

## How do I format SQL query strings in Python?

Write the query as a triple-quoted string and run `sqlty format .`. It lays out
the SQL with sqruff's layout rules, in your project's dialect, and changes only
whitespace, so the query means the same. `sqlty format --check .` fails CI when
a query is not formatted. See [Formatting queries](guide/formatting.md).

## Is sqlty a query builder or an ORM?

No. You write the SQL yourself, and sqlty checks it. It infers types, rewrites
the annotations in your source and lints the queries, but it never generates
SQL or maps tables to classes. See [Why SQLTy?](motivation.md).

## How does sqlty compare with asyncpg_typed?

asyncpg_typed checks types you declare by hand when asyncpg prepares each query
on a live PostgreSQL server. sqlty infers the types from casts or a schema and
checks them statically, with no database, for PostgreSQL, MySQL, SQLite and
DuckDB, and it also infers nullability. asyncpg_typed types the rows asyncpg
returns and compares `StrEnum` values with the database; sqlty does neither yet.
See [asyncpg_typed](motivation.md#asyncpg_typed).

## Which Python versions are supported?

Python 3.14 and 3.15. Install with `pip install sqlty` or `uv add sqlty`. See
[Installation](installation.md).

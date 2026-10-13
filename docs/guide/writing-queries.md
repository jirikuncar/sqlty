# Writing queries

sqlty works on string literals that start with `--sql` (or `-- sql`). It infers a row type
for each one and `sqlty rewrite` wraps the string in `SQL[T](...)`.

## The `--sql` prefix

Put `--sql` on its own line, then the query. In a single-line string, use the
`\n` (or `\r\n`) escape:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# rewritten: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

`--sql` is a SQL line comment, so the database ignores it. A space instead of
the newline turns the whole query into that comment: sqlty sees an empty
query and leaves the string alone (`sqlty check` reports
[`inline-sql-comment`](../reference/rules.md#inline-sql-comment)):

```python
query = "--sql SELECT id::INTEGER FROM users"
# rewritten: query = "--sql SELECT id::INTEGER FROM users"
```

Single quotes work the same way:

```python
query = '--sql\nSELECT id::INTEGER FROM users'
# rewritten: query = SQL[tuple[int]]('--sql\nSELECT id::INTEGER FROM users')
```

MySQL and MariaDB read `--` as a comment only before whitespace, so their
servers reject `--sql`. Use `-- sql` there: it is a comment in every dialect,
and sqlty reads it as the marker when only blanks or a `--` comment follow it
on its line (`"-- sql SELECT ..."` is an ordinary comment and left alone). With
`dialect = "mysql"`, `sqlty check` reports `--sql` in plain string literals as
[`mysql-sql-marker`](../reference/rules.md#mysql-sql-marker) and `--fix`
changes it.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
query = "-- sql\nSELECT CAST(id AS SIGNED) FROM users"
# rewritten: query = SQL[tuple[int]]("-- sql\nSELECT CAST(id AS SIGNED) FROM users")
```

## Multi-line queries

In a triple-quoted string, start the query on the line after `--sql`:

```python
query = """--sql
SELECT id::INTEGER, email::TEXT
FROM users
WHERE active
"""
# rewritten-file: from sqlty import SQL
# rewritten-file: query = SQL[tuple[int, str]]("""--sql
# rewritten-file: SELECT id::INTEGER, email::TEXT
# rewritten-file: FROM users
# rewritten-file: WHERE active
# rewritten-file: """)
```

Implicitly concatenated strings are one query, wrapped as a whole:

```python
query = ("--sql\nSELECT id::INTEGER, email::TEXT "
         "FROM users")
# rewritten: query = (SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, email::TEXT "
# rewritten:          "FROM users"))
```

The rewriter never changes the string itself, only the wrapper around it.

## Casts declare column types

Without a [schema](schema-inference.md), sqlty reads column types from
PostgreSQL-style `::TYPE` casts or from `CAST(x AS TYPE)`:

```python
query = "--sql\nSELECT created_at::TIMESTAMP, CAST(token AS UUID) FROM sessions"
# rewritten-file: import datetime
# rewritten-file: import uuid
# rewritten-file: from sqlty import SQL
# rewritten-file: query = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql\nSELECT created_at::TIMESTAMP, CAST(token AS UUID) FROM sessions")
```

The rewriter adds `import datetime`, `import uuid`, `import decimal`,
`import ipaddress` and `import typing` when the row type needs them. `SQL` and
`UNTYPED` already imported by any top-level `from sqlty import ...` (a list,
parentheses, `*`) are not imported again; a missing one gets a line of its own.

Literals, `COUNT(*)` and casts inside expressions are typed too. A column
without a cast, schema entry or literal type, a column of a type sqlty does not
know, and an expression it cannot type, such as arithmetic or `NULL`, make the
row type unknown: the string is left as it is (`SQL[UNTYPED]` with
[`untyped = "sentinel"`](untyped.md)) and `sqlty check` skips the column.
Predicates are `bool`, `||` is `str`, and arithmetic on typed operands promotes to the
wider type. In PostgreSQL, built-in functions such as `left`, `date_trunc`,
`greatest`, `unnest` and the advisory locks are typed too; `COALESCE`,
`GREATEST` and `LEAST` take the first argument of known type, skipping `NULL`
and placeholders. Cast the whole expression to type it:

```python
unknown = "--sql\nSELECT COUNT(*), 1, x::INTEGER + 1, name FROM t"
typed = "--sql\nSELECT COUNT(*), 1, (x::INTEGER + 1)::INTEGER, name::TEXT FROM t"
# rewritten: unknown = "--sql\nSELECT COUNT(*), 1, x::INTEGER + 1, name FROM t"
# rewritten: typed = SQL[tuple[int, int, int, str]]("--sql\nSELECT COUNT(*), 1, (x::INTEGER + 1)::INTEGER, name::TEXT FROM t")
```

An unaliased column is named as PostgreSQL names it: the column (also under a
cast), the function (`count`, `max`, `now`), a keyword such as `case` or
`exists`, else `?column?`. Alias columns that a row type matches by name.

Row types are never optional: a nullable column is typed as its base type.
Annotate a [row type](row-types.md) with `T | None` when you need it.

## Which statements are wrapped

Only statements that return rows get a row type. DML gets one when it has
`RETURNING`; DML without it, DDL, transaction control and invalid SQL stay
plain strings:

```python
deleted = "--sql\nDELETE FROM users WHERE id = 1 RETURNING id::INTEGER"
purge = "--sql\nDELETE FROM users WHERE id = 1"
broken = "--sql\nSELEC id FROM users"
# rewritten: deleted = SQL[tuple[int]]("--sql\nDELETE FROM users WHERE id = 1 RETURNING id::INTEGER")
# rewritten: purge = "--sql\nDELETE FROM users WHERE id = 1"
# rewritten: broken = "--sql\nSELEC id FROM users"
```

Queries that return rows of an unknown type (`VALUES`, unparseable `SELECT`)
are also left alone by default; see [Untyped queries](untyped.md) to mark them
explicitly.

## f-strings and t-strings

An f-string or t-string is built at runtime, so it cannot be rewritten. sqlty
skips every `--sql` string on a line that holds one:

```python
# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
query = f"--sql\nSELECT {column}::INTEGER FROM users"
# rewritten: query = f"--sql\nSELECT {column}::INTEGER FROM users"
```

`sqlty check` and `sqlty rewrite` report such strings as
[`fstring-skipped`](../reference/rules.md#fstring-skipped) and
[`tstring-skipped`](../reference/rules.md#tstring-skipped). Pass values as query
parameters instead of interpolating them, and keep the query a plain literal.

## Type mapping

| SQL type | Python type |
|----------|-------------|
| `INTEGER`, `INT`, `BIGINT`, `SMALLINT`, `TINYINT`, `MEDIUMINT`, `YEAR` | `int` |
| `UTINYINT`, `USMALLINT`, `UMEDIUMINT`, `UINT`, `UBIGINT` (MySQL `... UNSIGNED`) | `int` |
| `SIGNED`, `UNSIGNED` (MySQL `CAST(x AS SIGNED)`) | `int` |
| `TEXT`, `VARCHAR`, `CHAR`, `STRING` | `str` |
| `BOOLEAN`, `BOOL` | `bool` |
| `FLOAT`, `DOUBLE`, `REAL`, `UDOUBLE` (MySQL `... UNSIGNED`) | `float` |
| `DECIMAL`, `NUMERIC`, `UDECIMAL` (MySQL `... UNSIGNED`) | `decimal.Decimal`, `float` in `sqlite` and `oracle` |
| `DATE` | `datetime.date` |
| `TIMESTAMP`, `TIMESTAMPTZ`, `DATETIME` | `datetime.datetime` |
| `TIME`, `TIMETZ` | `datetime.time` |
| `INTERVAL` | `datetime.timedelta` |
| `JSON`, `JSONB` | `typing.Any`: any JSON value can be stored |
| `INET` | `ipaddress.IPv4Address \| ipaddress.IPv6Address \| ipaddress.IPv4Interface \| ipaddress.IPv6Interface` |
| `CIDR` | `ipaddress.IPv4Network \| ipaddress.IPv6Network` |
| `UUID` | `uuid.UUID` |
| `BYTEA`, `BLOB`, `BINARY`, `VARBINARY` | `bytes` |
| `SERIAL`, `SMALLSERIAL`, `BIGSERIAL` | `int` |
| `ENUM` (MySQL), `CITEXT`, `NAME`, `BPCHAR`, `XML`, `MONEY`, `MACADDR`, `TSVECTOR`, `TSQUERY` | `str` |
| Arrays: `INT[]`, `INT[3]`, `INT ARRAY`, `INT ARRAY[3]` | `list[T]` with the element type mapped, `list[list[int]]` for `INT[][]` |
| Anything else (`SET`, `POINT`, custom types, ...) | unknown: the row type is unknown |

### Dialect notes

- The types are the ones psycopg returns: `NUMERIC` is `decimal.Decimal`, `inet`
  an address, or an interface when the value has a prefix (`1.2.3.4/24`).
  `AVG` of integers, `SUM` without a known argument type and fractional
  literals (`1.5`) are decimals too. Drivers for SQLite and Oracle return
  floats, so `decimal.Decimal` is used in the other dialects, MySQL included.
- A `TEXT[]` column gives `list[str]`. Narrowing the element type by hand (a
  `list[str]` field for `list[int]`) is not reported: elements are not compared.
  A JSON column gives `Any`, which any field accepts, and any annotation
  (`dict`, `list[str]`, a `TypedDict`) in `SQL[tuple[...]]` is not `stale-type`.
- An array type needs an element type: `x::INT[]` is a `list[int]`, but a bare
  `x::ARRAY` is rejected and nothing is inferred. A fixed size must be an
  integer literal (`INT[3]`, not `INT[n]`); the size itself is ignored.
- MySQL has no `INT[]`; use `CAST(x AS UNSIGNED ARRAY)`.
- In MySQL, `... UNSIGNED` becomes `U<type>` (`INT UNSIGNED` is `UINT`),
  `FLOAT UNSIGNED` becomes `UDOUBLE`, and `INT3`/`MIDDLEINT` (`MEDIUMINT`) and
  `INT8` (`BIGINT`) are aliases. `UNSIGNED` applies to `DECIMAL`/`DEC`, not to
  `NUMERIC` or `FIXED`.
- `SET(...)`, `SIGNED INTEGER` and `UNSIGNED INTEGER` are MySQL-only. In other
  dialects they are rejected, and a schema `CREATE TABLE` that uses one is
  skipped entirely, so `SELECT *` from that table cannot be expanded.

See [Schema inference](schema-inference.md#dialects) for choosing a dialect.

## Editing queries

`sqlty rewrite` never replaces an existing wrapper, so after you change the
SQL inside `SQL[T](...)`, `T` stays as it was. There are three ways to update
it.

### Edit in place

Change the SQL inside the wrapper and run `sqlty check`. It reports
[`stale-type`](../reference/rules.md#stale-type) when a `SQL[tuple[...]]` no
longer matches the query, and the
[row type rules](../reference/rules.md#row-type-rules) when a named
[row type](row-types.md) no longer fits:

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int, str]] should be SQL[tuple[int, str, str]]"
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
```

`sqlty check --fix` then writes the inferred `T` into every stale
`SQL[tuple[...]]` and `SQL[UNTYPED]`, keeping generics you parametrised by
hand (`list[str]`); row types you wrote by hand are left for you to update:

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str, str]]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
```

This works for every file, including ones with row types you wrote by hand.

### Strip and rewrite

For files whose wrappers are all `SQL[tuple[...]]`, let sqlty regenerate
them:

```bash
sqlty strip queries.py     # SQL[T]("--sql\n...") -> "--sql\n..."
# edit the SQL
sqlty rewrite queries.py   # "--sql\n..." -> SQL[T]("--sql\n...")
```

Neither command changes the string contents, only the wrapper and the
`from sqlty import SQL` import. For a file whose `--sql` strings are all
plain, `strip(rewrite(x)) == x`, unless it already has a hand-written import
line that `strip` removes too: `from sqlty import SQL`,
`from sqlty import UNTYPED`, or the configured `import-style` line.

!!! warning "`strip` removes hand-written row types"
    `strip` removes every wrapper, including `SQL[User]`, and the sqlty
    import even if you wrote it yourself. The next `rewrite` produces
    `SQL[tuple[...]]`, so `User` is lost. Do not strip files with row types.

```python
user = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# stripped: user = "--sql\nSELECT id::INTEGER FROM users"
```

## Import style

`sqlty rewrite` writes `from sqlty import SQL` and `SQL[T](...)` by default.
[`import-style`](../reference/configuration.md#import-style) changes both:

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
query = "--sql\nSELECT id::INTEGER FROM users"
# rewritten-file: from sqlty import SQL as Q
# rewritten-file: query = Q[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

Example project:
[`examples/05_import_style/`](https://github.com/jirikuncar/sqlty/tree/main/examples/05_import_style).

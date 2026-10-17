---
name: sqlty
description: Write, type and check raw SQL in Python with sqlty. Use when adding or editing a query in a project that uses sqlty (strings starting with --sql, SQL[T] or SQLTemplate[T] wrappers, a [tool.sqlty] section), when generating Python row types for a query, or when checking that queries still fit a schema migration before deploying.
---

# sqlty

sqlty infers the Python row type of each SQL query from the query and the
project's schema, writes it as `SQL[T]`, and checks it. `SQL[T]` is a `str`
subclass: it works wherever a string query works.

Read `[tool.sqlty]` in `pyproject.toml` first. `dialect` (default `postgres`)
and `schema` (SQL files with `CREATE TABLE`, or migrations) decide every type.

## Write a typed query

1. Write the query as a string whose first line is the marker `--sql`, followed by
   a newline: `"--sql\nSELECT ..."`. With `"--sql SELECT ..."` (a space) the whole
   query is a SQL comment and nothing is checked.
2. Run `sqlty rewrite <file>`. It wraps the string as `SQL[T]("--sql\n...")`, with
   `T` the inferred row type, and adds the import.
3. Run `sqlty check <file>`. It must report nothing.

```python
user_id = 1
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = %s"
# rewritten: user_id = 1
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = %s")
```

A query `rewrite` leaves alone has a column sqlty cannot type: a column of a
table the schema lacks, an unknown function, `SELECT *` with no schema. Cast it
(`x::INTEGER`) or fix the schema; don't hand-write a guessed `T`.

## Name the row type

Replace the tuple with a `TypedDict`, `dataclass`, `NamedTuple` or pydantic model
whose fields are the columns, in order. `sqlty check` compares each field's name,
type and nullability with the query:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# error: [column-type-mismatch] "User.name is str but query column 'name' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::INTEGER AS name FROM users")
```

A column that can be NULL needs `T | None`. Run `sqlty check --fix` to update
stale tuple types after editing a query; it never edits a named row type.

## Check

| Goal | Command |
|------|---------|
| Lint everything | `sqlty check .` |
| Fail CI on warnings | `sqlty check --error-on-warning .` |
| Fix what is fixable | `sqlty check --fix .` |
| Report names the schema lacks | `sqlty check --warn unknown-table --warn unknown-column .` |
| See which rule typed a column | `sqlty check --explain .` |
| Learn about a rule | `sqlty rule <name>` |

To inspect one query without a file, use the Python API:

```python
from sqlty.core import analyze_params, analyze_select, infer_type

schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT)"
infer_type("SELECT id, name FROM users", schema_sql=schema)  # 'tuple[int, str | None]'
analyze_params("SELECT name FROM users WHERE id = %s", schema_sql=schema)  # %s is an int
```

## Check a schema change before deploying

```bash
sqlty breaking --against origin/main
```

It checks the queries of `origin/main` against the schema of the working tree
(migrate first) and the new queries against the old schema (deploy first), and
exits 1 on a dropped or renamed table or column, a new `NOT NULL` column an
`INSERT` leaves out, or a row type that no longer fits. Use
`--direction backward` when migrations always run before the code ships.

## Pitfalls

- `SQL[T]` only types the string; pass it to the driver as usual, or use
  `sqlty.drivers.psycopg.execute` to get typed rows.
- Building a query with `+`, `%` or an f-string loses the type and is not
  checked (`fstring-skipped`). Pass values as parameters (`%s`, `$1`, `?`).
- t-strings are checked as `SQLTemplate[T]`; psycopg's `{x:i}`, `{x:l}` and
  `{part:q}` fields are read in modules that import psycopg.
- Types follow the dialect's driver: in SQLite a `DATE` is `str` and a `NUMERIC`
  a `float`. See `docs/reference/types.md` before "fixing" a type.

## Reference

- Writing queries, t-strings, the type mapping: `docs/guide/writing-queries.md`, `docs/reference/types.md`
- Row types: `docs/guide/row-types.md`
- Parameters: `docs/guide/parameters.md`
- Schema changes: `docs/guide/breaking-changes.md`
- Every rule: `docs/reference/rules.md`, or `sqlty rule`

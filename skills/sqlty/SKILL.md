---
name: sqlty
description: Write, type and check raw SQL in Python with sqlty. Use when adding or editing a query in a project that uses sqlty (strings starting with --sql, SQL[T] or SQLTemplate[T] wrappers, a [tool.sqlty] section), when generating Python row types for a query, or when checking that queries still fit a schema migration before deploying. Routes to sqlty-rewrite, sqlty-check, sqlty-breaking, sqlty-ci and sqlty-adopt for each task.
---

# sqlty

sqlty infers the Python row type of each SQL query from the query and the
project's schema, writes it as `SQL[T]`, and checks it. `SQL[T]` is a `str`
subclass: it works wherever a string query works.

Run `sqlty version`: this skill may be newer or older than the installed sqlty.
When it and `sqlty --help` or `sqlty rule NAME` disagree, trust the command.

Read `[tool.sqlty]` in `pyproject.toml` first. `dialect` (default `postgres`)
and `schema` (SQL files with `CREATE TABLE`, or migrations) decide every type.

## Pick the task

| Task | Skill |
|------|-------|
| Write or edit a query; add or remove `SQL[T]` wrappers | `sqlty-rewrite` |
| Read a diagnostic, name a row type, check parameters | `sqlty-check` |
| Change a table, write a migration, plan a rename | `sqlty-breaking` |
| Set up or repair pre-commit or CI | `sqlty-ci` |
| Run sqlty in VS Code or another editor | `sqlty-server` |
| Introduce sqlty into a project that doesn't use it | `sqlty-adopt` |

`sqlty skill NAME` prints a skill that isn't installed.

## The `--sql` marker

sqlty reads only strings whose first line is `--sql`, followed by a newline.
With a space, the whole query is a SQL comment and nothing is checked:

```python
right = "--sql\nSELECT id::INTEGER FROM users"
wrong = "--sql SELECT id::INTEGER FROM users"
# rewritten: right = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten: wrong = "--sql SELECT id::INTEGER FROM users"
```

In a triple-quoted string, end the first line after `--sql`:

```python
q = """--sql
SELECT id::INTEGER, name::TEXT FROM users"""
# rewritten: q = SQL[tuple[int, str]]("""--sql
# rewritten: SELECT id::INTEGER, name::TEXT FROM users""")
```

## The loop

```bash
sqlty rewrite app/queries.py   # wrap new --sql strings as SQL[T]
sqlty check app/               # report what doesn't fit
```

Never hand-write a guessed `T`: let `rewrite` infer it, then name it (see
`sqlty-check`). A query `rewrite` leaves alone has a column sqlty cannot type.

## Pitfalls

- `SQL[T]` only types the string; pass it to the driver as usual, or use
  `sqlty.drivers.psycopg.execute` to get typed rows.
- Building a query with `+`, `%` or an f-string loses the type and is not
  checked (`fstring-skipped`). Pass values as parameters (`%s`, `$1`, `?`).
- t-strings are checked as `SQLTemplate[T]`; psycopg's `{x:i}`, `{x:l}` and
  `{part:q}` fields are read in modules that import psycopg.
- Types follow the dialect's driver: in SQLite a `DATE` is `str` and a `NUMERIC`
  a `float`. See [Type mapping](https://jirikuncar.github.io/sqlty/reference/types/) before "fixing" a type.

## Reference

The documentation is at <https://jirikuncar.github.io/sqlty/>:

- [Writing queries](https://jirikuncar.github.io/sqlty/guide/writing-queries/), t-strings included, and the
  [type mapping](https://jirikuncar.github.io/sqlty/reference/types/)
- [Row types](https://jirikuncar.github.io/sqlty/guide/row-types/)
- [Parameters](https://jirikuncar.github.io/sqlty/guide/parameters/)
- [Schema changes](https://jirikuncar.github.io/sqlty/guide/breaking-changes/)
- Every rule: `sqlty rule NAME` prints its page; [all rules](https://jirikuncar.github.io/sqlty/reference/rules/)

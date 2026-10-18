# insert-missing-column

Default severity: **error**

## What it does

Reports an `INSERT` with a column list, or MySQL's `INSERT ... SET`, that leaves
out a schema column declared `NOT NULL` without a default. A `DEFAULT`,
`serial`, identity, generated or auto-increment column, and SQLite's
`INTEGER PRIMARY KEY`, get a value anyway.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
insert-missing-column = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    bio TEXT
);
```

```python
# error: [insert-missing-column] "INSERT into `users` leaves out `name`, which is NOT NULL and has no default"
q = "--sql\nINSERT INTO users (email) VALUES (%s)"
```

## Why is this bad?

The database rejects the row: the column has no value and may not be NULL. A
migration that adds such a column breaks every `INSERT` written before it, which
`sqlty breaking` reports.

## How to fix

Set the column in the `INSERT`, or give it a `DEFAULT` in the schema.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
insert-missing-column = "warn"
```

Needs a [schema](../../../docs/guide/schema-inference.md), and is judged in the
`postgres`, `mysql` and `sqlite` dialects. An `INSERT` without a column list
is not checked.

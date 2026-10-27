# insert-missing-column

Default severity: **error**

## What it does

Reports an `INSERT` that leaves out a schema column declared `NOT NULL` without
a default. Without a column list, the values fill the table's columns in order:
PostgreSQL gives the columns past them their defaults, and so do `DEFAULT VALUES`
and MySQL's `VALUES ()`. A `DEFAULT`,
`serial`, identity, generated or auto-increment column, and SQLite's
`INTEGER PRIMARY KEY`, get a value anyway. So does every column of a PostgreSQL
or MySQL table with a `BEFORE INSERT ... FOR EACH ROW` trigger, which may set it.

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
# error: [insert-missing-column] "INSERT into `users` leaves out `name`, which is NOT NULL and has no default"
r = "--sql\nINSERT INTO users VALUES (DEFAULT, %s)"
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
from a query whose columns sqlty can't count is not checked.

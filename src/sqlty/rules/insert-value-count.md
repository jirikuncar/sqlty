# insert-value-count

Default severity: **error**

## What it does

Reports an `INSERT` whose `VALUES` rows, or query, have a different number of
values than the columns it writes. Without a column list it writes the table's
columns in order: MySQL and SQLite want a value for each, and PostgreSQL at most
one for each.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    name TEXT
);
```

```python
# error: [insert-value-count] "INSERT into `users` gives 1 value for 2 columns"
q = "--sql\nINSERT INTO users (email, name) VALUES (%s)"
# error: [insert-value-count] "INSERT into `users` gives 2 values for 1 column"
r = "--sql\nINSERT INTO users (email) SELECT email, name FROM users"
# error: [insert-value-count] "INSERT into `users` gives 4 values for 3 columns"
s = "--sql\nINSERT INTO users VALUES (DEFAULT, %s, %s, %s)"
```

## Why is this bad?

The database rejects the statement. PostgreSQL reports `INSERT has more target
columns than expressions`, MySQL `Column count doesn't match value count` and
SQLite `2 values for 1 columns`.

## How to fix

Give one value for each column the `INSERT` names, or name the columns the
values are for.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
insert-value-count = "warn"
```

Needs a [schema](../../../docs/guide/schema-inference.md) and is judged in the
`postgres`, `mysql` and `sqlite` dialects. A query source whose columns sqlty
can't count, such as `SELECT *` over a table it doesn't know, is not checked.

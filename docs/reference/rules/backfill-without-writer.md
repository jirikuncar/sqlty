---
order: 457
group: "migration"
severity: "warn"
fixable: false
summary: "Migration backfills a column that the new code does not write"
origin: "Migration"
---

# backfill-without-writer

Default severity: **warn**

## What it does

Reports a new statement that fills a column of a table that has rows (an
`UPDATE ... SET column`), or copies rows into a table the migrations create (an
`INSERT INTO table ... SELECT`), while a query of the code `sqlty breaking` checks
inserts into that table without the column, or updates a column the backfilled value
is computed from without updating the column too.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT NOT NULL);
```

`app/q.py`:

```python
from sqlty import SQL

signup = SQL("--sql\nINSERT INTO users (id, name) VALUES (%s, %s)")
```

```sql migration
SET lock_timeout = '1s';
ALTER TABLE users ADD COLUMN display_name TEXT;
-- error: [backfill-without-writer] "fills `users.display_name`, which `app/q.py:3` inserts into `users` without: rows it writes after the backfill have no `display_name`; deploy code that writes `display_name` first, then backfill"
UPDATE users SET display_name = name WHERE id IN (SELECT id FROM users WHERE display_name IS NULL ORDER BY id LIMIT 5000);
```

## Why is this bad?

A backfill copies the rows that exist when it runs. Every row the old code path
writes afterwards misses the column: a user who signs up after the backfill has no
`display_name`, and code that reads it next finds NULL. A rename after the backfill
leaves `display_name` with the old name. The safe order is to write
the new column from the application first, and backfill only once every writer is
deployed ([Stripe, Online migrations at scale](https://stripe.com/blog/online-migrations)).

## How to fix

Write the column in every query that inserts into the table, and in every update of
what it is computed from, deploy that code, then run the backfill:

`app/q.py`:

```python
from sqlty import SQL

signup = SQL("--sql\nINSERT INTO users (id, name, display_name) VALUES (%s, %s, %s)")
```

```sql migration
SET lock_timeout = '1s';
ALTER TABLE users ADD COLUMN display_name TEXT;
-- error:
UPDATE users SET display_name = name WHERE id IN (SELECT id FROM users WHERE display_name IS NULL ORDER BY id LIMIT 5000);
```

A column with a `DEFAULT`, or a generated one, gets a value without the code: it is
not reported. Neither is an `INSERT` into a table with a `BEFORE INSERT` row trigger,
nor an `UPDATE` of one with a `BEFORE UPDATE` row trigger, which may set the column.

## Configuration

A warning by default: the backfill succeeds, the rows written after it are what
lack the value. Raise or turn it off:

```toml
[tool.sqlty.rules]
backfill-without-writer = "error"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have, against the queries of `--head` that use that schema: the `SQL(...)` and
`--sql` strings of its Python files and the queries of its `.sql` query files. No
query of the code that writes the table means nothing to compare with, so nothing is
reported. Queries passed to helper functions are not read. `-- sqlty: ignore[backfill-without-writer]`
on the first line of a statement, or alone on its line above the statement, drops it.

---
order: 445
group: "migration"
severity: "error"
fixable: false
summary: "Migration adds a NOT NULL column without a default to a table"
origin: "Migration"
---

# add-required-column

Default severity: **error**

## What it does

Reports a new `ADD COLUMN ... NOT NULL` without a `DEFAULT` on a table that
already exists.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY);
```

```sql migration
SET lock_timeout = '2s';
-- error: [add-required-column] "adding `name` as NOT NULL without a DEFAULT fails when `users` has rows: add it nullable or with a default"
ALTER TABLE users ADD COLUMN name TEXT NOT NULL;
```

## Why is this bad?

Every existing row would get NULL, which the column forbids, so PostgreSQL
rejects the statement on any table that has rows. It passes on an empty
development database and fails in production. Code that inserts rows without the
column fails too, which `sqlty breaking` reports as
[`insert-missing-column`](insert-missing-column.md).

## How to fix

Add the column with a constant `DEFAULT`, which PostgreSQL 11 and later store
without a rewrite, or add it nullable, backfill, and set `NOT NULL` without a
scan as [`constraint-scan`](constraint-scan.md) shows.

## Configuration

An error by default: the statement fails on a table with rows. Lower or turn it off:

```toml
[tool.sqlty.rules]
add-required-column = "warn"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[add-required-column]` on the first line of a statement drops it.

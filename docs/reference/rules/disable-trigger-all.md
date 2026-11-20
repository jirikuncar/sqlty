---
order: 450
group: "migration"
severity: "warn"
fixable: false
summary: "Migration disables every trigger of a table, foreign key checks included"
origin: "Migration"
---

# disable-trigger-all

Default severity: **warn**

## What it does

Reports a new `ALTER TABLE ... DISABLE TRIGGER ALL` on a table that has rows
and a foreign key, to or from it, or that the schema does not define.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY);
CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT REFERENCES users (id));
```

```sql migration
SET lock_timeout = '2s';
-- error: [disable-trigger-all] "`DISABLE TRIGGER ALL` also turns off the foreign key checks of `orders`: rows written meanwhile are never checked; disable `USER` triggers or name them"
ALTER TABLE orders DISABLE TRIGGER ALL;
ALTER TABLE orders ENABLE TRIGGER ALL;
```

## Why is this bad?

`ALL` includes the [internal triggers that enforce foreign keys](https://www.postgresql.org/docs/17/sql-altertable.html#SQL-ALTERTABLE-DESC-DISABLE-ENABLE-TRIGGER).
Every row written while they are off, by the migration or by the application,
skips the check, and enabling them again does not check those rows. Only a
superuser may disable them, so a migration that runs as another role fails
instead.

## How to fix

Disable only the triggers the load must skip:

```sql migration
SET lock_timeout = '2s';
-- error:
ALTER TABLE orders DISABLE TRIGGER USER;
ALTER TABLE orders ENABLE TRIGGER USER;
```

## Configuration

```toml
[tool.sqlty.rules]
disable-trigger-all = "error"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[disable-trigger-all]` on the first line of a statement, or alone on the line above it, drops it.

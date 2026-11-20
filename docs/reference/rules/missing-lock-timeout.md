---
order: 446
group: "migration"
severity: "warn"
fixable: false
summary: "Migration takes a blocking lock without a lock timeout"
origin: "Migration"
---

# missing-lock-timeout

Default severity: **warn**

## What it does

Reports the first new statement of a migration file that takes a lock that
blocks writes to a table that has rows (`ALTER TABLE`, `DROP TABLE`,
`DROP INDEX`, `LOCK TABLE`, `TRUNCATE`) before the file sets `lock_timeout`. Once per file.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY);
```

```sql migration
-- error: [missing-lock-timeout] "takes a lock that blocks writes without `SET lock_timeout` first: while it waits for the lock, the queries it blocks wait behind it"
ALTER TABLE users ADD COLUMN bio TEXT;
```

```sql migration
SET lock_timeout = '2s';
-- error:
ALTER TABLE users ADD COLUMN bio TEXT;
```

## Why is this bad?

Even an instant `ALTER TABLE` waits for its lock behind any long query of the
table, and every query that comes after it queues behind the waiting `ALTER`:
the table looks down until the long query ends.

## How to fix

Start the migration with `SET lock_timeout = '2s';` (or a value that suits your
traffic) and retry the migration when it times out.

## Configuration

A warning by default: the migration tool may set `lock_timeout` on its connection, which sqlty can't see. Lower or turn it off:

```toml
[tool.sqlty.rules]
missing-lock-timeout = "ignore"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[missing-lock-timeout]` on the first line of a statement, or alone on the line above it, drops it.

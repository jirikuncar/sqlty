---
order: 451
group: "migration"
severity: "warn"
fixable: false
summary: "Migration locks several tables against writes in one transaction"
origin: "Migration"
---

# multi-table-lock

Default severity: **warn**

## What it does

Reports the first new statement of a transaction that locks a second table that
has rows against writes (`SHARE` or a stronger lock: most `ALTER TABLE` forms,
a foreign key on both tables, `CREATE INDEX` without `CONCURRENTLY`, `TRUNCATE`,
`DROP TABLE`, `LOCK TABLE`), while the transaction still holds the first. Once
per transaction.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY);
CREATE TABLE orders (id BIGINT PRIMARY KEY);
```

```sql migration
BEGIN;
SET LOCAL lock_timeout = '2s';
ALTER TABLE users ADD COLUMN last_seen_at TIMESTAMPTZ;
-- error: [multi-table-lock] "locks `orders` while this transaction still holds `users`: queries of any of them now wait for all of them, until `COMMIT`; change one table per transaction"
ALTER TABLE orders ADD COLUMN channel TEXT;
COMMIT;
```

## Why is this bad?

A transaction keeps every lock it takes until it ends. Each `ALTER TABLE`
waits for the queries of its table, and holds the locks it already has while
it waits: one slow query on any of the tables stalls the queries of all of
them, and the more tables, the likelier a deadlock with the application.

## How to fix

Change one table per transaction. Ignore the rule on a statement that needs
both tables at once, such as `TRUNCATE` of tables linked by a foreign key or
renames that swap two tables:

```sql migration
SET lock_timeout = '2s';
-- error:
ALTER TABLE users ADD COLUMN last_seen_at TIMESTAMPTZ;
ALTER TABLE orders ADD COLUMN channel TEXT;
```

## Configuration

```toml
[tool.sqlty.rules]
multi-table-lock = "ignore"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[multi-table-lock]` on the first line of a statement drops it.

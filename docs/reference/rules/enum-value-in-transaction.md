---
order: 448
group: "migration"
severity: "error"
fixable: false
summary: "Migration uses an enum value in the transaction that added it"
origin: "Migration"
---

# enum-value-in-transaction

Default severity: **error**

## What it does

Reports the first new statement that uses an enum value which
`ALTER TYPE ... ADD VALUE` added earlier in the same transaction.

`against.sql`:

```sql
CREATE TYPE order_status AS ENUM ('paid', 'shipped');
CREATE TABLE orders (id BIGINT PRIMARY KEY, status order_status, refunded_at TIMESTAMPTZ);
```

```sql migration
SET lock_timeout = '2s';
BEGIN;
ALTER TYPE order_status ADD VALUE 'refunded';
-- error: [enum-value-in-transaction] "'refunded' was added to `order_status` in this transaction: PostgreSQL refuses a new enum value until the transaction that added it commits, so `COMMIT` after `ADD VALUE`"
UPDATE orders SET status = 'refunded' WHERE id IN (SELECT id FROM orders WHERE refunded_at IS NOT NULL LIMIT 5000);
COMMIT;
```

## Why is this bad?

PostgreSQL adds the value, but [refuses to use it](https://www.postgresql.org/docs/17/sql-altertype.html)
until the transaction that added it commits: the statement fails with
`unsafe use of new value`, and the migration rolls back.

## How to fix

Commit after `ADD VALUE`, then use the value:

```sql migration
SET lock_timeout = '2s';
BEGIN;
ALTER TYPE order_status ADD VALUE 'refunded';
COMMIT;
-- error:
UPDATE orders SET status = 'refunded' WHERE id IN (SELECT id FROM orders WHERE refunded_at IS NOT NULL LIMIT 5000);
```

## Configuration

An error by default: the migration fails at this statement.

```toml
[tool.sqlty.rules]
enum-value-in-transaction = "warn"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A statement uses the value when the literal casts to the type, or the
statement names a column of the type or no table the schema knows; the label is
compared case-sensitively. `-- sqlty: ignore[enum-value-in-transaction]` on the
first line of a statement, or alone on the line above it, drops it.

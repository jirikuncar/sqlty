---
order: 452
group: "migration"
severity: "warn"
fixable: false
summary: "Migration backfills a table its transaction has locked"
origin: "Migration"
---

# locked-backfill

Default severity: **warn**

## What it does

Reports the first new `UPDATE` or `DELETE` of a table that has rows after an
earlier statement of the same transaction locked it against writes. Once per
transaction.

`against.sql`:

```sql
CREATE TABLE orders (id BIGINT PRIMARY KEY, country_code TEXT);
```

```sql migration
BEGIN;
SET LOCAL lock_timeout = '2s';
ALTER TABLE orders ADD COLUMN region TEXT;
-- error: [locked-backfill] "an earlier statement of this transaction locks `orders` against writes, so they wait for the whole backfill: `COMMIT` the schema change first"
UPDATE orders SET region = 'eu' WHERE id IN (SELECT id FROM orders WHERE region IS NULL LIMIT 5000);
COMMIT;
```

## Why is this bad?

`ALTER TABLE` takes an `ACCESS EXCLUSIVE` lock, and the transaction keeps it
until `COMMIT`: every read and write of the table waits for the backfill,
however it is batched.

## How to fix

Commit the schema change, then backfill in later transactions:

```sql migration
SET lock_timeout = '2s';
ALTER TABLE orders ADD COLUMN region TEXT;
-- error:
UPDATE orders SET region = 'eu' WHERE id IN (SELECT id FROM orders WHERE region IS NULL LIMIT 5000);
```

## Configuration

```toml
[tool.sqlty.rules]
locked-backfill = "error"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[locked-backfill]` on the first line of a statement drops it.

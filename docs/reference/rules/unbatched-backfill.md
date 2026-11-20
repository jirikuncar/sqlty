---
order: 453
group: "migration"
severity: "warn"
fixable: false
summary: "Migration updates or deletes the rows of a table in one statement"
origin: "Migration"
---

# unbatched-backfill

Default severity: **warn**

## What it does

Reports a new `UPDATE` or `DELETE` of a table that has rows without `LIMIT`
or `BETWEEN`, which a batched backfill uses to bound each statement.

`against.sql`:

```sql
CREATE TABLE orders (id BIGINT PRIMARY KEY, country_code TEXT, region TEXT);
```

```sql migration
-- error: [unbatched-backfill] "changes every matching row of `orders` in one statement: it holds their row locks and grows the WAL until it ends; change them in batches by key, `WHERE id IN (SELECT id ... LIMIT 5000)`"
UPDATE orders SET region = 'eu' WHERE country_code = 'DE';
```

## Why is this bad?

One statement changes every matching row in one transaction: it holds a row
lock on each until it ends, so the application's writes to those rows wait,
and it writes a new version of each row and its WAL at once, which replicas
must replay before they catch up.

## How to fix

Change a bounded batch per statement and repeat until none is left, from a
script or a migration that runs outside a transaction:

```sql migration
-- error:
UPDATE orders SET region = 'eu'
WHERE id IN (SELECT id FROM orders WHERE country_code = 'DE' AND region IS NULL ORDER BY id LIMIT 5000);
```

## Configuration

A small table is safe to change at once: ignore the statement with
`-- sqlty: ignore[unbatched-backfill]`, or turn the rule off.

```toml
[tool.sqlty.rules]
unbatched-backfill = "ignore"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it.

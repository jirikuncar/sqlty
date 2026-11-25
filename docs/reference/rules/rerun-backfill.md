---
order: 455
group: "migration"
severity: "warn"
fixable: false
summary: "Migration copies rows in a way that fails or duplicates when run again"
origin: "Migration"
---

# rerun-backfill

Default severity: **warn**

## What it does

Reports a new `INSERT ... SELECT` into a table that has rows without
`ON CONFLICT`, unless the query reads the target table, as a `NOT EXISTS` or
an anti-join on its key does to skip the rows already copied.

`against.sql`:

```sql
CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT NOT NULL);
CREATE TABLE user_stats (user_id BIGINT PRIMARY KEY, orders BIGINT NOT NULL);
```

```sql migration
-- error: [rerun-backfill] "copies rows into `user_stats` without `ON CONFLICT`: run again after a timeout or a restart, it fails on the key or adds the rows twice; add `ON CONFLICT (key) DO NOTHING`, or `DO UPDATE ... WHERE` to keep newer rows"
INSERT INTO user_stats (user_id, orders)
SELECT user_id, count(*) FROM orders WHERE user_id BETWEEN 1 AND 5000 GROUP BY user_id;
```

## Why is this bad?

A backfill gets run again: after a lock or statement timeout, or a deploy that
restarts the job. The second run inserts the same rows. With a unique key it
fails on the first one it copied before, and the job never finishes; without
one, every row is there twice.

## How to fix

Skip or update the rows already there with `ON CONFLICT`:

```sql migration
-- error:
INSERT INTO user_stats (user_id, orders)
SELECT user_id, count(*) FROM orders WHERE user_id BETWEEN 1 AND 5000 GROUP BY user_id
ON CONFLICT (user_id) DO NOTHING;
```

Or leave them out of the query:

```sql migration
-- error:
INSERT INTO user_stats (user_id, orders)
SELECT user_id, count(*) FROM orders o
WHERE user_id BETWEEN 1 AND 5000
  AND NOT EXISTS (SELECT 1 FROM user_stats s WHERE s.user_id = o.user_id)
GROUP BY user_id;
```

## Configuration

A migration tool that never runs a statement twice makes it moot: turn the
rule off.

```toml
[tool.sqlty.rules]
rerun-backfill = "ignore"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[rerun-backfill]` on the first line of a statement, or alone on its line above the statement, drops it.

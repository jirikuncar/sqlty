---
order: 441
group: "migration"
severity: "warn"
fixable: false
summary: "Migration builds or drops an index without CONCURRENTLY"
origin: "Migration"
---

# missing-concurrently

Default severity: **warn**

## What it does

Reports a new migration statement that builds or drops an index of a table that
already has rows without `CONCURRENTLY`: `CREATE INDEX`, `DROP INDEX`, `REINDEX`,
`ADD UNIQUE` or `ADD PRIMARY KEY` without `USING INDEX`, `DETACH PARTITION`, and
`REFRESH MATERIALIZED VIEW`. An index of a partitioned table is not reported:
PostgreSQL builds none `CONCURRENTLY`, so build it on each partition, then
`CREATE INDEX ... ON ONLY` the parent and attach them.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT, bio TEXT);
CREATE INDEX users_bio ON users (bio);
```

```sql migration
SET lock_timeout = '2s';
-- error: [missing-concurrently] "`CREATE INDEX` blocks writes to `users` until the index is built: create it `CONCURRENTLY`"
CREATE INDEX users_email ON users (email);
-- error: [missing-concurrently] "`DROP INDEX` waits for an ACCESS EXCLUSIVE lock on its table"
DROP INDEX users_bio;
-- error: [missing-concurrently] "the constraint builds its index under an ACCESS EXCLUSIVE lock on `users`"
ALTER TABLE users ADD CONSTRAINT users_email_key UNIQUE (email);
```

The same statements, run `CONCURRENTLY`, are not reported:

```sql migration
-- error:
CREATE INDEX CONCURRENTLY users_email ON users (email);
DROP INDEX CONCURRENTLY users_bio;
CREATE UNIQUE INDEX CONCURRENTLY users_email_key ON users (email);
SET lock_timeout = '2s';
ALTER TABLE users ADD CONSTRAINT users_email_key UNIQUE USING INDEX users_email_key;
```

## Why is this bad?

A plain `CREATE INDEX` takes a `SHARE` lock: reads go on, but every `INSERT`,
`UPDATE` and `DELETE` waits until the index is built, which grows with the table.
`DROP INDEX`, `DETACH PARTITION` and a constraint that builds its own index take
`ACCESS EXCLUSIVE`, which blocks reads too. A plain `REFRESH MATERIALIZED VIEW`
blocks every read of the view while it runs.

## How to fix

Add `CONCURRENTLY`. For `UNIQUE` and `PRIMARY KEY`, build the index with
`CREATE UNIQUE INDEX CONCURRENTLY`, then add the constraint `USING INDEX`. A
concurrent build can't run inside a transaction block: see
[`concurrently-in-transaction`](concurrently-in-transaction.md).

## Configuration

An error by default: writes to the table stop for as long as the index takes. Lower or turn it off:

```toml
[tool.sqlty.rules]
missing-concurrently = "warn"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[missing-concurrently]` on the first line of a statement drops it.

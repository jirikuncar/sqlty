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
`DROP INDEX`, `LOCK TABLE`, `TRUNCATE`, `CREATE INDEX`, `REINDEX` and
`REFRESH MATERIALIZED VIEW` without `CONCURRENTLY`, `VACUUM FULL`, `CLUSTER`)
before the file sets `lock_timeout`. Once per file.

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
the table looks down until the long query ends. An index build waiting for its
SHARE lock queues writes the same way.

## How to fix

Start the migration with `SET lock_timeout = '2s';` (or a value that suits your
traffic) and retry the migration when it times out.

## Configuration

A warning by default: the migration tool may set `lock_timeout` on its connection, which sqlty can't see. Lower or turn it off:

```toml
[tool.sqlty.rules]
missing-lock-timeout = "ignore"
```

### `max`

A long timeout keeps the problem it exists to prevent: every query of the table
queues behind the waiting statement for that long. Set `max`, and a
`SET [LOCAL] lock_timeout` above it counts as unset:

```toml
[tool.sqlty.rules.missing-lock-timeout]
max = "2s"
```

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY);
```

```sql migration
SET lock_timeout = '30s';
-- error: [missing-lock-timeout] "takes a lock that blocks writes with `lock_timeout` 30s, above the configured maximum of 2s"
ALTER TABLE users ADD COLUMN bio TEXT;
```

```sql migration
SET lock_timeout = '1500ms';
-- error:
ALTER TABLE users ADD COLUMN bio TEXT;
```

`max` is a duration as PostgreSQL reads `lock_timeout`: a number of `ms`, `s`,
`min`, `h` or `d`, milliseconds without a unit. There is no default, so a
configuration without it reports a missing timeout only.

### `severity`

In the table form, `severity` sets the severity:

```toml
[tool.sqlty.rules.missing-lock-timeout]
severity = "error"
max = "500ms"
```

!!! tip "Stay below `deadlock_timeout` when a migration takes several locks"
    `deadlock_timeout` is 1 s by default. A `max` below it makes the lock timeout
    fire before deadlock detection runs, so the migration fails fast instead of
    holding one lock while it waits for the next
    ([client settings](https://www.postgresql.org/docs/17/runtime-config-client.html#GUC-LOCK-TIMEOUT),
    [lock management](https://www.postgresql.org/docs/17/runtime-config-locks.html#GUC-DEADLOCK-TIMEOUT)).

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[missing-lock-timeout]` on the first line of a statement, or alone on its line above the statement, drops it.

---
order: 449
group: "migration"
severity: "warn"
fixable: false
summary: "Migration builds an index CONCURRENTLY with IF NOT EXISTS"
origin: "Migration"
---

# concurrent-index-if-not-exists

Default severity: **warn**

## What it does

Reports a new `CREATE INDEX CONCURRENTLY IF NOT EXISTS` on a table that has rows.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT);
```

```sql migration
-- error: [concurrent-index-if-not-exists] "a failed `CREATE INDEX CONCURRENTLY` leaves an invalid index on `users`, and `IF NOT EXISTS` then skips building it again: `DROP INDEX CONCURRENTLY IF EXISTS` first and create it without `IF NOT EXISTS`"
CREATE INDEX CONCURRENTLY IF NOT EXISTS users_email ON users (email);
```

## Why is this bad?

A concurrent build that fails, on a deadlock or a duplicate key, [leaves the
index behind marked invalid](https://www.postgresql.org/docs/17/sql-createindex.html#SQL-CREATEINDEX-CONCURRENTLY).
When the migration runs again, `IF NOT EXISTS` finds that index and succeeds
without building it: queries never use it, and writes still maintain it.

## How to fix

Drop a leftover first, then build:

```sql migration
-- error:
DROP INDEX CONCURRENTLY IF EXISTS users_email;
CREATE INDEX CONCURRENTLY users_email ON users (email);
```

## Configuration

```toml
[tool.sqlty.rules]
concurrent-index-if-not-exists = "error"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[concurrent-index-if-not-exists]` on the first line of a statement drops it.

---
order: 442
group: "migration"
severity: "error"
fixable: false
summary: "Migration runs CONCURRENTLY inside a transaction block"
origin: "Migration"
---

# concurrently-in-transaction

Default severity: **error**

## What it does

Reports a new statement that runs `CONCURRENTLY` between `BEGIN` (or
`START TRANSACTION`) and `COMMIT` in the same migration file.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT);
```

```sql migration
BEGIN;
-- error: [concurrently-in-transaction] "PostgreSQL refuses `CONCURRENTLY` inside a transaction block: run it in a migration without one"
CREATE INDEX CONCURRENTLY users_email ON users (email);
COMMIT;
```

## Why is this bad?

PostgreSQL refuses `CREATE INDEX CONCURRENTLY`, `DROP INDEX CONCURRENTLY`,
`REINDEX CONCURRENTLY` and `DETACH PARTITION ... CONCURRENTLY` inside a
transaction block, so the migration fails.

## How to fix

Run the statement outside the transaction. Alembic's `--sql` output marks
`autocommit_block()` with `COMMIT` before and `BEGIN` after it; Django needs
`atomic = False` on the migration.

## Configuration

An error by default: the migration fails at this statement. Lower or turn it off:

```toml
[tool.sqlty.rules]
concurrently-in-transaction = "warn"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[concurrently-in-transaction]` on the first line of a statement drops it.

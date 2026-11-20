---
order: 444
group: "migration"
severity: "warn"
fixable: false
summary: "Migration rewrites a table under an ACCESS EXCLUSIVE lock"
origin: "Migration"
---

# table-rewrite

Default severity: **warn**

## What it does

Reports a new statement that rewrites a table that already has rows:
`ADD COLUMN` with a volatile default (`gen_random_uuid()`, `random()`,
`clock_timestamp()`, `nextval()`...), a `serial` type, an identity column or a
stored generated column; `ALTER COLUMN ... TYPE` to a type that needs a rewrite;
`SET LOGGED`, `SET UNLOGGED` and `SET TABLESPACE`; `VACUUM FULL` and `CLUSTER`.

`against.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, email VARCHAR(100), score NUMERIC(5, 2));
```

```sql migration
SET lock_timeout = '2s';
-- error: [table-rewrite] "adding `token` rewrites `users` under an ACCESS EXCLUSIVE lock: its default calls gen_random_uuid(), which is volatile, so each row gets its own value"
ALTER TABLE users ADD COLUMN token UUID DEFAULT gen_random_uuid();
-- error: [table-rewrite] "changing `id` to BIGINT rewrites `users` and its indexes under an ACCESS EXCLUSIVE lock"
ALTER TABLE users ALTER COLUMN id TYPE BIGINT;
-- error: [table-rewrite] "`VACUUM FULL` rewrites the table under an ACCESS EXCLUSIVE lock"
VACUUM FULL users;
```

A constant or stable default (`now()`) is stored once, not per row, and a type
change PostgreSQL can make without touching the rows is not reported: a longer
or unbounded `varchar`, `varchar` to `text`, `numeric` with more precision and
the same scale, or `cidr` to `inet`:

```sql migration
SET lock_timeout = '2s';
-- error:
ALTER TABLE users ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now();
ALTER TABLE users ALTER COLUMN email TYPE TEXT;
ALTER TABLE users ALTER COLUMN score TYPE NUMERIC(9, 2);
```

## Why is this bad?

A rewrite copies every row to new storage, and rebuilds the indexes, while it
holds `ACCESS EXCLUSIVE`: no query of the table runs until it is done.

## How to fix

Add the column without the default, backfill it in batches, then set the
default. To change a type, add a new column, write both, backfill, and switch
reads over. Rebuild a bloated table online with `pg_repack` or `pg_squeeze`, outside
migrations.

## Configuration

An error by default: the table can't be read or written until every row is copied. Lower or turn it off:

```toml
[tool.sqlty.rules]
table-rewrite = "warn"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[table-rewrite]` on the first line of a statement drops it.

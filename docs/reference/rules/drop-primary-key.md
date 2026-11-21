---
order: 454
group: "migration"
severity: "warn"
fixable: false
summary: "Migration leaves a table without a replica identity"
origin: "Migration"
---

# drop-primary-key

Default severity: **warn**

## What it does

Reports a new statement that drops the primary key of a table that has rows,
when the table has no other replica identity, and `REPLICA IDENTITY NOTHING`.
Inside a transaction it is reported at `COMMIT`, unless the transaction gives
the table a new primary key or replica identity first.

`against.sql`:

```sql
CREATE TABLE orders (id BIGINT PRIMARY KEY, uuid UUID NOT NULL);
CREATE UNIQUE INDEX orders_uuid ON orders (uuid);
```

```sql migration
SET lock_timeout = '2s';
-- error: [drop-primary-key] "`orders` is left without a replica identity: if it is published for logical replication, every `UPDATE` and `DELETE` of it fails until it has one; set `REPLICA IDENTITY USING INDEX` on a unique index first, or add the new primary key in the same transaction"
ALTER TABLE orders DROP CONSTRAINT orders_pkey;
```

Replaced in one transaction, or after `REPLICA IDENTITY USING INDEX`, the table
always has an identity:

```sql migration
BEGIN;
SET LOCAL lock_timeout = '2s';
-- error:
ALTER TABLE orders DROP CONSTRAINT orders_pkey;
ALTER TABLE orders ADD CONSTRAINT orders_pkey PRIMARY KEY USING INDEX orders_uuid;
COMMIT;
```

## Why is this bad?

Logical replication finds the subscriber's copy of an updated or deleted row by
its replica identity, the primary key by default. Without one, PostgreSQL
refuses every `UPDATE` and `DELETE` of a table in a publication that replicates
them, so the application's writes fail until a new key is in place.

## How to fix

Add the new primary key in the same transaction, or set
`REPLICA IDENTITY USING INDEX` on a unique index of `NOT NULL` columns before
dropping the old one.

## Configuration

```toml
[tool.sqlty.rules]
drop-primary-key = "error"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[drop-primary-key]` on the first line of a statement, or alone on its line above the statement, drops it.

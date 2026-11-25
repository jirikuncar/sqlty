---
order: 456
group: "migration"
severity: "warn"
fixable: false
summary: "Migration drops a foreign key with an ON DELETE or ON UPDATE action"
origin: "Migration"
---

# dropped-delete-action

Default severity: **warn**

## What it does

Reports a new statement that drops a foreign key with `ON DELETE` or
`ON UPDATE` `CASCADE`, `SET NULL` or `SET DEFAULT`: `DROP CONSTRAINT`,
`DROP COLUMN` of one of its columns, or `DROP TABLE ... CASCADE` of the table it
references. Not when the migration adds a key with the same columns, table and
actions back, or drops the table that has the key.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY);
CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT REFERENCES users (id) ON DELETE CASCADE);
```

```sql migration
SET lock_timeout = '1s';
-- error: [dropped-delete-action] "drops the foreign key of `orders` (user_id) to `users` with `ON DELETE CASCADE`: changing a row of `users` no longer changes its rows of `orders`; add a key with the same action first, or do it in the application"
ALTER TABLE orders DROP CONSTRAINT orders_user_id_fkey;
```

## Why is this bad?

The action is behavior, not only a name. No query reads it, so
`sqlty breaking` finds no reader to report. After the migration, deleting a user
leaves its orders behind, and nothing fails.

## How to fix

Replace the key in the same migration: add the new one `NOT VALID` with the
same action, validate it, then drop the old one.

```sql migration
SET lock_timeout = '1s';
ALTER TABLE orders ADD CONSTRAINT orders_user FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE NOT VALID;
-- error:
ALTER TABLE orders VALIDATE CONSTRAINT orders_user;
ALTER TABLE orders DROP CONSTRAINT orders_user_id_fkey;
```

When the application deletes the rows itself now, ignore the statement with
`-- sqlty: ignore[dropped-delete-action]`.

## Configuration

```toml
[tool.sqlty.rules]
dropped-delete-action = "ignore"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. `-- sqlty: ignore[dropped-delete-action]` on the first line of a statement, or alone on its line above the statement, drops it.

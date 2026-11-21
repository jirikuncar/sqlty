---
order: 447
group: "migration"
severity: "warn"
fixable: false
summary: "Migration truncates with CASCADE"
origin: "Migration"
---

# truncate-cascade

Default severity: **warn**

## What it does

Reports a new `TRUNCATE ... CASCADE` of a table that has rows and is referenced
by a foreign key, or that the schema does not define.

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY);
CREATE TABLE sessions (id BIGINT PRIMARY KEY, user_id BIGINT REFERENCES users (id));
```

```sql migration
SET lock_timeout = '2s';
-- error: [truncate-cascade] "`TRUNCATE ... CASCADE` also empties every table with a foreign key to `users`, and the tables that reference those: truncate the tables by name"
TRUNCATE users CASCADE;
```

## Why is this bad?

`CASCADE` empties every table that references the truncated one, through
any chain of foreign keys, including tables added after the migration was
written. Each of them is locked `ACCESS EXCLUSIVE` until the transaction ends.

## How to fix

Name every table to empty in one `TRUNCATE`, so a new reference makes it
fail instead of losing rows:

```sql migration
SET lock_timeout = '2s';
-- error:
TRUNCATE sessions, users;
```

## Configuration

```toml
[tool.sqlty.rules]
truncate-cascade = "error"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[truncate-cascade]` on the first line of a statement, or alone on its line above the statement, drops it.

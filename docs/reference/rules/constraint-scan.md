---
order: 443
group: "migration"
severity: "warn"
fixable: false
summary: "Migration checks every row of a table while it blocks writes"
origin: "Migration"
---

# constraint-scan

Default severity: **warn**

## What it does

Reports a new statement that makes PostgreSQL check every existing row while it
holds a lock that blocks writes: a `FOREIGN KEY` or `CHECK` constraint added
without `NOT VALID`, and `SET NOT NULL` on a column without a validated
`CHECK (column IS NOT NULL)`.

`against.sql`:

```sql
CREATE TABLE teams (id BIGINT PRIMARY KEY);
CREATE TABLE users (id BIGINT PRIMARY KEY, team_id BIGINT, bio TEXT);
```

```sql migration
SET lock_timeout = '2s';
-- error: [constraint-scan] "adding the constraint checks every row of `users` while it blocks writes: add it `NOT VALID`, then `VALIDATE CONSTRAINT` it"
ALTER TABLE users ADD CONSTRAINT users_team FOREIGN KEY (team_id) REFERENCES teams (id);
-- error: [constraint-scan] "`SET NOT NULL` scans `users` for NULLs under an ACCESS EXCLUSIVE lock"
ALTER TABLE users ALTER COLUMN bio SET NOT NULL;
```

Added `NOT VALID` and validated in a later statement, neither blocks writes
while rows are checked; PostgreSQL 12 and later use the validated check for
`SET NOT NULL`:

```sql migration
SET lock_timeout = '2s';
-- error:
ALTER TABLE users ADD CONSTRAINT users_team FOREIGN KEY (team_id) REFERENCES teams (id) NOT VALID;
ALTER TABLE users VALIDATE CONSTRAINT users_team;
ALTER TABLE users ADD CONSTRAINT users_bio CHECK (bio IS NOT NULL) NOT VALID;
ALTER TABLE users VALIDATE CONSTRAINT users_bio;
ALTER TABLE users ALTER COLUMN bio SET NOT NULL;
```

## Why is this bad?

A validated foreign key locks both tables against writes while every row is
checked; a `CHECK` constraint and `SET NOT NULL` hold `ACCESS EXCLUSIVE`, which
blocks reads too. `VALIDATE CONSTRAINT` checks the rows under `SHARE UPDATE
EXCLUSIVE`, which lets reads and writes go on.

## How to fix

Add the constraint `NOT VALID`, then `VALIDATE CONSTRAINT` it as a separate
statement. For `SET NOT NULL`, validate a `CHECK (column IS NOT NULL)` first;
drop it afterwards if you like.

## Configuration

An error by default: writes stop while the whole table is read. Lower or turn it off:

```toml
[tool.sqlty.rules]
constraint-scan = "warn"
```

Judged by [`sqlty breaking`](../../guide/breaking-changes.md#migrations-that-lock-tables)
in the `postgres` dialect, on the statements of the
[schema](../../guide/schema-inference.md) files that the `--against` baseline does
not have. A table created earlier in the migrations is new and empty, so it is not
reported until rows are copied into it. `-- sqlty: ignore[constraint-scan]` on the first line of a statement drops it.

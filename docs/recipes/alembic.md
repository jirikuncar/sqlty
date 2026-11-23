---
title: "Lint Alembic migrations for locks and rewrites"
description: "How to check Alembic migrations for blocking locks, table rewrites and failing DDL with sqlty breaking, from the SQL that alembic upgrade --sql writes, without a database."
---

# Alembic

Alembic writes the SQL of its migrations without a database in
[offline mode](https://alembic.sqlalchemy.org/en/latest/offline.html). Commit
that SQL as the schema, and sqlty types your queries against it and lints each
new migration for [locks that stop a table](../migration-safety.md).

## Write the schema

```bash
alembic upgrade head --sql > db/schema.sql
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "db/schema.sql"
```

The file holds every migration in order, with the `BEGIN` and `COMMIT` Alembic
runs them in and a `-- Running upgrade 0001 -> 0002` comment before each.
`sqlty check` reads its `CREATE TABLE` and `ALTER TABLE` statements as the
schema of your queries.

Regenerate it in the pull request that adds a migration. In CI, fail when it is
stale:

```bash
alembic upgrade head --sql > db/schema.sql
git diff --exit-code db/schema.sql
```

## Lint new migrations

```bash
sqlty breaking --against origin/main
```

The statements of `db/schema.sql` that `origin/main` does not have are the new
migrations. Here `origin/main` ends at revision `0002`:

`against.sql`:

```sql
BEGIN;
SET lock_timeout = '2s';
CREATE TABLE alembic_version (version_num VARCHAR(32) NOT NULL, CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num));
-- Running upgrade  -> 0001
CREATE TABLE users (id BIGSERIAL NOT NULL, email TEXT NOT NULL, PRIMARY KEY (id));
INSERT INTO alembic_version (version_num) VALUES ('0001') RETURNING alembic_version.version_num;
-- Running upgrade 0001 -> 0002
ALTER TABLE users ADD COLUMN plan TEXT DEFAULT 'free' NOT NULL;
UPDATE alembic_version SET version_num='0002' WHERE alembic_version.version_num = '0001';
COMMIT;
```

and the pull request adds `0003`, which builds an index without `CONCURRENTLY`:

```sql migration
BEGIN;
SET lock_timeout = '2s';
CREATE TABLE alembic_version (version_num VARCHAR(32) NOT NULL, CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num));
-- Running upgrade  -> 0001
CREATE TABLE users (id BIGSERIAL NOT NULL, email TEXT NOT NULL, PRIMARY KEY (id));
INSERT INTO alembic_version (version_num) VALUES ('0001') RETURNING alembic_version.version_num;
-- Running upgrade 0001 -> 0002
ALTER TABLE users ADD COLUMN plan TEXT DEFAULT 'free' NOT NULL;
UPDATE alembic_version SET version_num='0002' WHERE alembic_version.version_num = '0001';
-- Running upgrade 0002 -> 0003
-- error: [missing-concurrently] "`CREATE INDEX` blocks writes to `users` until the index is built: create it `CONCURRENTLY`"
CREATE INDEX users_plan ON users (plan);
UPDATE alembic_version SET version_num='0003' WHERE alembic_version.version_num = '0002';
COMMIT;
```

```text
db/schema.sql:11: [missing-concurrently] `CREATE INDEX` blocks writes to `users` until the index is built: create it `CONCURRENTLY` (migration: a statement the working tree runs that origin/main does not have)
```

Build the index in an `autocommit_block()`, outside the migration's transaction:

```python
def upgrade() -> None:
    with op.get_context().autocommit_block():
        op.create_index("users_plan", "users", ["plan"], postgresql_concurrently=True)
```

Alembic writes `COMMIT` before the block and `BEGIN` after it, so
[`concurrently-in-transaction`](../reference/rules/concurrently-in-transaction.md)
sees the index built outside a transaction:

```sql migration
BEGIN;
SET lock_timeout = '2s';
CREATE TABLE alembic_version (version_num VARCHAR(32) NOT NULL, CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num));
-- Running upgrade  -> 0001
CREATE TABLE users (id BIGSERIAL NOT NULL, email TEXT NOT NULL, PRIMARY KEY (id));
INSERT INTO alembic_version (version_num) VALUES ('0001') RETURNING alembic_version.version_num;
-- Running upgrade 0001 -> 0002
ALTER TABLE users ADD COLUMN plan TEXT DEFAULT 'free' NOT NULL;
UPDATE alembic_version SET version_num='0002' WHERE alembic_version.version_num = '0001';
-- Running upgrade 0002 -> 0003
COMMIT;
-- error:
CREATE INDEX CONCURRENTLY users_plan ON users (plan);
BEGIN;
UPDATE alembic_version SET version_num='0003' WHERE alembic_version.version_num = '0002';
COMMIT;
```

## Set a lock timeout

[`missing-lock-timeout`](../reference/rules/missing-lock-timeout.md) reports a
lock that blocks writes taken before `SET lock_timeout`. Set it in `env.py`, in
both `run_migrations_offline()` and `run_migrations_online()`, so the SQL you
lint is the SQL you run:

```python
with context.begin_transaction():
    context.execute("SET lock_timeout = '2s'")
    context.run_migrations()
```

## Migrations that read data

A migration that reads rows through `op.get_bind()` can't run with `--sql`:
offline mode has no connection. Write the change as SQL with `op.execute()`, or
skip the read when `context.is_offline_mode()` is true. To start from a later
revision, `alembic upgrade 1975ea83b712:head --sql` writes only the migrations
after it; tables created before it are then unknown to `sqlty check`.

See [Zero-downtime PostgreSQL migrations](../migration-safety.md) for every
rule, and [Breaking schema changes](../guide/breaking-changes.md) for what else
`sqlty breaking` checks.

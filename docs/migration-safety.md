---
title: "Zero-downtime PostgreSQL migration linting"
description: "How sqlty breaking lints new PostgreSQL migrations for blocking locks, table rewrites and failing DDL from your schema files, and how it compares with Squawk."
---

# Zero-downtime PostgreSQL migrations

```bash
sqlty breaking --against origin/main
```

A migration can be compatible with every query and still take the site down: an
`ALTER TABLE` that waits for a lock queues every query behind it, and one that
rewrites the table holds that lock until each row is copied. With the `postgres`
dialect, [`sqlty breaking`](guide/breaking-changes.md) judges each statement of
your migrations that the `--against` baseline does not have, before it runs. It
reads the files only: no database, no migration tool.

## An example

`against.sql` is the schema the baseline ends with, the statements below are the
new migration:

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT NOT NULL, total NUMERIC NOT NULL);
```

```sql migration
SET lock_timeout = '2s';
-- error: [add-required-column] "adding `plan` as NOT NULL without a DEFAULT fails when `users` has rows: add it nullable or with a default"
ALTER TABLE users ADD COLUMN plan TEXT NOT NULL;
-- error: [constraint-scan] "adding the constraint checks every row of `orders` while it blocks writes: add it `NOT VALID`, then `VALIDATE CONSTRAINT` it"
ALTER TABLE orders ADD CONSTRAINT orders_user FOREIGN KEY (user_id) REFERENCES users (id);
-- error: [unbatched-backfill] "changes every matching row of `orders` in one statement: it holds their row locks and grows the WAL until it ends; change them in batches by key, `WHERE id IN (SELECT id ... LIMIT 5000)`"
UPDATE orders SET total = 0;
```

The fixes keep each lock short: add the column with a default, add the foreign
key `NOT VALID` and validate it in a second statement, which lets writes through,
and backfill in batches.

```sql migration
SET lock_timeout = '2s';
-- error:
ALTER TABLE users ADD COLUMN plan TEXT NOT NULL DEFAULT 'free';
ALTER TABLE orders ADD CONSTRAINT orders_user FOREIGN KEY (user_id) REFERENCES users (id) NOT VALID;
ALTER TABLE orders VALIDATE CONSTRAINT orders_user;
UPDATE orders SET total = 0 WHERE id BETWEEN 1 AND 10000;
```

## The rules

**Locks that stop the table.** A statement that blocks writes, or reads too, for
as long as it runs:

| Rule | Reports |
|------|---------|
| [`missing-concurrently`](reference/rules/missing-concurrently.md) | an index built or dropped without `CONCURRENTLY` |
| [`table-rewrite`](reference/rules/table-rewrite.md) | a statement that rewrites the table: a volatile default, a type change that is not binary coercible, `VACUUM FULL` |
| [`constraint-scan`](reference/rules/constraint-scan.md) | a constraint, `SET NOT NULL` or a new partition that checks every row while it blocks writes |
| [`missing-lock-timeout`](reference/rules/missing-lock-timeout.md) | a lock that blocks writes taken before `SET lock_timeout` |
| [`multi-table-lock`](reference/rules/multi-table-lock.md) | a transaction that locks several tables against writes |
| [`locked-backfill`](reference/rules/locked-backfill.md) | an `UPDATE` or `DELETE` of a table its transaction locked |
| [`unbatched-backfill`](reference/rules/unbatched-backfill.md) | an `UPDATE` or `DELETE` whose `WHERE` does not bound its rows |

**Statements that fail.** PostgreSQL rejects them on a table with rows, or in a
transaction:

| Rule | Reports |
|------|---------|
| [`add-required-column`](reference/rules/add-required-column.md) | a `NOT NULL` column without a default |
| [`concurrently-in-transaction`](reference/rules/concurrently-in-transaction.md) | `CONCURRENTLY` or `VACUUM` between `BEGIN` and `COMMIT` |
| [`enum-value-in-transaction`](reference/rules/enum-value-in-transaction.md) | an enum value used in the transaction that added it |
| [`concurrent-index-if-not-exists`](reference/rules/concurrent-index-if-not-exists.md) | `CREATE INDEX CONCURRENTLY IF NOT EXISTS`, which skips an invalid index a failed build left |

**Data and replication.** Statements that do more than they say:

| Rule | Reports |
|------|---------|
| [`truncate-cascade`](reference/rules/truncate-cascade.md) | `TRUNCATE ... CASCADE` |
| [`disable-trigger-all`](reference/rules/disable-trigger-all.md) | `DISABLE TRIGGER ALL`, which turns off foreign key checks |
| [`drop-primary-key`](reference/rules/drop-primary-key.md) | a dropped primary key or `REPLICA IDENTITY NOTHING`, leaving a table without a replica identity |
| [`rerun-backfill`](reference/rules/rerun-backfill.md) | an `INSERT ... SELECT` without `ON CONFLICT`, which fails or duplicates rows when run again |

A dropped or renamed column is not a rule of its own: `sqlty breaking` reports it
where a query of the other revision still reads it. See
[Breaking schema changes](guide/breaking-changes.md).

## What the schema tells it

A linter that sees one migration file can't tell an empty table from one with a
billion rows, or a `varchar` column from an `integer` one. `sqlty breaking` knows
the schema each statement changes, from the baseline:

- Only new statements are judged. The migrations that already ran are never
  reported again, and need no `ignore` comment.
- A table the new statements create is empty: locking it is harmless until rows
  are copied into it with `INSERT ... SELECT`, `COPY ... FROM`, `MERGE` or
  `CREATE TABLE ... AS` without `WITH NO DATA`.
- A type change is judged by the column's current type: `varchar(50)` to `text`,
  or `numeric(5, 2)` to `numeric(9, 2)`, is metadata only; `integer` to `bigint`
  rewrites the table.
- `SET NOT NULL` is not reported when a validated `CHECK (col IS NOT NULL)`
  already proves it.

`add-required-column`, `concurrently-in-transaction` and
`enum-value-in-transaction` are errors by default, so `sqlty breaking` fails on
them; the others warn. To fail CI on every one, raise them for your migrations:

```toml
[[tool.sqlty.overrides]]
include = ["migrations/**"]

[tool.sqlty.overrides.rules]
missing-concurrently = "error"
table-rewrite = "error"
constraint-scan = "error"
```

Severities, `-- sqlty: ignore[rule]` comments and `[tool.sqlty.overrides]` work as for
every other rule. [Migrations that lock tables](guide/breaking-changes.md#migrations-that-lock-tables)
covers which files are read and how each database of a configuration is judged.

## Where the migrations come from

`schema` names the files `sqlty breaking` reads. Point it at your SQL
migrations, or at the SQL your migration tool writes for them:

| Tool | Schema files |
|------|--------------|
| Plain SQL, Flyway | the migration directory |
| Alembic | the output of `alembic upgrade head --sql`; see the [Alembic recipe](recipes/alembic.md) |
| A `pg_dump --schema-only` dump | not judged: it describes the result, not the steps |

[Schema changes in CI](recipes/migration.md#schema-changes-in-ci) shows how to
run `sqlty breaking` on every pull request.

## SQLTy vs Squawk

[Squawk](https://github.com/sbdchd/squawk) is the established PostgreSQL
migration linter: a Rust CLI, language server and formatter that lints the SQL
files you pass it. The two overlap on lock safety and differ on what they know:

| | Squawk 2.74 | SQLTy |
|-|-------------|-------|
| Reads | the SQL files or stdin you pass | the new statements of the `schema` files, against a baseline revision or snapshot |
| Knows the existing schema | no: only tables created in the same transaction | yes: every table and column of the baseline |
| Type changes | every `ALTER COLUMN ... TYPE` ([`changing-column-type`](https://squawkhq.com/docs/changing-column-type)) | only those that rewrite the table, from the column's current type |
| Dropped columns | every drop ([`ban-drop-column`](https://squawkhq.com/docs/ban-drop-column)) | a drop a query of the other revision still reads |
| Rules | over 70, including schema style (`prefer-text-field`, `prefer-bigint-over-int`, `prefer-timestamptz`) | 14 migration rules, plus the query and row type rules of `sqlty check` |
| Transactions | follows `BEGIN` and `COMMIT`; `assume_in_transaction` for files a tool wraps in one | follows `BEGIN` and `COMMIT` |
| Suppression | `-- squawk-ignore rule` | `-- sqlty: ignore[rule]` on or above the statement |
| PostgreSQL version | `--pg-version` | not configurable |
| Python code | not read | queries in Python checked against both schemas |

Use Squawk for its wider set of style and `ban-*` rules on any SQL file. Use
SQLTy when the migrations live next to Python code that queries the same tables:
one tool judges the migration and the code that has to keep working through it.
They can run side by side.

## Sources

- [Squawk rules](https://squawkhq.com/docs/rules), [CLI and `.squawk.toml`](https://squawkhq.com/docs/cli)
  and [Alembic usage](https://squawkhq.com/docs/web-frameworks), read at Squawk 2.74.0
- PostgreSQL: [`ALTER TABLE` notes](https://www.postgresql.org/docs/current/sql-altertable.html#SQL-ALTERTABLE-NOTES)
  and [explicit locking](https://www.postgresql.org/docs/current/explicit-locking.html)

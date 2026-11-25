---
name: sqlty-breaking
description: Check that a schema change or migration doesn't break the queries running during a rolling deploy, with sqlty breaking, and plan an expand and contract migration. Use when adding, dropping, renaming or changing a table or column, writing a migration, making a column nullable or NOT NULL, reading a sqlty breaking finding (backward or forward), or splitting a schema change into safe pull requests.
---

# sqlty breaking

During a rolling deploy, the code of one revision runs against the schema of
another. `sqlty breaking` checks both mixes before you deploy:

```bash
sqlty breaking --against origin/main
```

| Direction | Checks | Happens when you |
|-----------|--------|------------------|
| backward | the queries of `--against` on the schema of `--head` | migrate first: old code, new schema |
| forward | the queries of `--head` on the schema of `--against` | deploy first: new code, old schema |

Run `sqlty version`: this skill may be newer or older than the installed sqlty.
When it and `sqlty breaking --help` disagree, trust the command.

`--head` defaults to the working tree, uncommitted changes included. Each
revision is read with its own `pyproject.toml`, `schema` and Python files. It
exits 0 when both mixes work, 1 when a change breaks one with an `error`
diagnostic, and 2 on an error such as an unknown revision. Rules and severities
follow `[tool.sqlty.rules]`, as in `sqlty check`; a `warn` finding is printed
without failing the run. Use `--direction backward` when migrations always
run before the deploy, `--direction forward` when the code always ships first.

## Read a finding

```text
app/q.py:2: [unknown-column] column `bio` does not exist in `users` (backward: the queries of origin/main against the schema of the working tree)
```

| Finding | Means | Fix |
|---------|-------|-----|
| backward `unknown-column`, `unknown-table` | the new schema drops or renames what running code reads | keep the old name until no deployed code reads it (contract later) |
| backward `insert-missing-column` | a new `NOT NULL` column without a default breaks running `INSERT`s | add it nullable or with a `DEFAULT`, tighten it later |
| backward `narrow-type`, `nullable-mismatch` | a column became nullable under code that expects a value | widen the code to `T \| None` first |
| forward `unknown-column`, `unknown-table` | new code reads what the old schema doesn't have yet | add the schema in an earlier pull request |

Only what the other schema breaks is reported: a diagnostic a query already gets
against its own schema is not repeated, and neither is `stale-rewrite` or
`unused-ignore`.

## Migrations that lock tables

With the `postgres` dialect, each schema statement the baseline doesn't have is
judged too, as a migration that runs on tables with rows:

```text
migrations/0042_index.sql:2: [missing-concurrently] `CREATE INDEX` blocks writes to `users` until the index is built: create it `CONCURRENTLY` (migration: a statement the working tree runs that origin/main does not have)
```

| Rule | Fix |
|------|-----|
| `missing-concurrently` | `CREATE INDEX CONCURRENTLY`, `DROP INDEX CONCURRENTLY`; `UNIQUE` via `CREATE UNIQUE INDEX CONCURRENTLY` then `ADD CONSTRAINT ... USING INDEX` |
| `concurrently-in-transaction` | move the statement out of `BEGIN` ... `COMMIT` (Django `atomic = False`, Alembic `autocommit_block()`) |
| `constraint-scan` | add the constraint `NOT VALID`, then `VALIDATE CONSTRAINT` after `COMMIT`; for `SET NOT NULL`, validate a `CHECK (col IS NOT NULL)` first; before `ATTACH PARTITION`, make the key `NOT NULL` and validate a `CHECK` matching the bounds |
| `table-rewrite` | add the column without a volatile default and backfill; change a type through a new column |
| `add-required-column` | add the column nullable or with a constant `DEFAULT` |
| `missing-lock-timeout` | start the migration with `SET lock_timeout = '2s';`, under 1 s when it takes several locks (see below) |
| `truncate-cascade` | name every table to empty: `TRUNCATE a, b;` |
| `enum-value-in-transaction` | `COMMIT` after `ALTER TYPE ... ADD VALUE`, then use the value |
| `concurrent-index-if-not-exists` | `DROP INDEX CONCURRENTLY IF EXISTS`, then `CREATE INDEX CONCURRENTLY` without `IF NOT EXISTS` |
| `disable-trigger-all` | `DISABLE TRIGGER USER` or name the triggers |
| `multi-table-lock` | one table per transaction |
| `locked-backfill` | `COMMIT` the schema change, then backfill |
| `unbatched-backfill` | update in batches: `WHERE id IN (SELECT id ... LIMIT 5000)` |
| `drop-primary-key` | add the new key in the same transaction, or `REPLICA IDENTITY USING INDEX` first |
| `rerun-backfill` | `ON CONFLICT (key) DO NOTHING`, or skip copied rows with `NOT EXISTS` |
| `dropped-delete-action` | add a key with the same `ON DELETE` action first, or delete the rows in the application |

Every `schema` of the configuration is judged, each `[tool.sqlty.databases]` entry
included, with the severities of `rules` and of the `[[tool.sqlty.overrides]]` that
match the migration's path (`include = ["migrations/**"]`, relative to the
configuration's directory, so for migrations outside it set `rules` or use an override
without `include`). A schema several configurations name is judged once, at the
strictest severity, against the baseline schema of each file checked against it (or
of the database of the same name), and each statement is reported once.

## A dropped column

Below, the code was written for `schema.sql`, and `against.sql` is the schema
it is checked against.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, bio TEXT);
```

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL);
```

```python
from sqlty import SQL

# error: [unknown-column] "column `bio` does not exist in `users`"
bios = SQL[tuple[str | None]]("--sql\nSELECT bio FROM users")
emails = SQL[tuple[str]]("--sql\nSELECT email FROM users")
```

## Expand and contract

Split a breaking change into pull requests that each pass `sqlty check` and
`sqlty breaking` in both directions, and deploy each before the next. Renaming
`users.name` to `display_name`:

| Pull request | Schema | Code |
|--------------|--------|------|
| 1. Widen the code | unchanged | annotate `name` as `str \| None`, with `# sqlty: ignore[stale-type]` |
| 2. Expand the schema | add `display_name`, nullable or with a `DEFAULT`; relax `name` to nullable | drop the `ignore` |
| 3. Expand the code | unchanged | write both columns, read `display_name` |
| 4. Migrate the data | backfill `display_name` | unchanged |
| 5. Contract the code | unchanged | read and write only `display_name` |
| 6. Contract the schema | drop `name`; make `display_name` `NOT NULL` | `sqlty check --fix` narrows `display_name` to `str` |

- **Widen the code before the schema.** Code annotated `str | None` reads the
  rows of both schemas. Against the old schema `stale-type` says it could be
  `str`: suppress it on that line until the schema changes.
- **Schema before the code that uses it.** Code that writes a column in the same
  pull request that adds it fails the forward check.
- **Contract the code before the schema.** Stop reading and writing a column
  before you drop it, or the running code fails.

Adding a column that nothing reads yet needs only steps 2 and 3. Dropping one
needs steps 5 and 6.

## Move data between columns or tables

`sqlty breaking` compares two revisions. It can't see the release order, the
row counts, or which code is deployed, so a data migration also follows these
rules:

| Step | Rule | Why |
|------|------|-----|
| Readers first | Ship code that reads both the old and the new shape one release before code writes the new one | during a rollout, old code reads what new code writes |
| Dual write | Write the new column or table in the same transaction as the old one, before any backfill | a row written after the backfill is never copied ([#1260](https://github.com/jirikuncar/sqlty/issues/1260)) |
| Backfill last | Run the backfill once the dual-writing release is fully deployed, from a job the deploy doesn't wait for | every writer is live before it reads |
| Small batches | Batch by key, one transaction per batch, `SET LOCAL lock_timeout` and `statement_timeout`, a short pause, `SKIP LOCKED` | `unbatched-backfill` and `locked-backfill` report the rest |
| Rerunnable | `ON CONFLICT ... DO UPDATE ... WHERE` the stored row is older; a rerun writes nothing | a retried batch must not fail or overwrite newer rows; `rerun-backfill` reports an `INSERT ... SELECT` without `ON CONFLICT` |
| Switch reads | Read the new source and fall back to the old one while the new one can lag | a missed write must not change an answer |

One batch, starting after the last key of the batch before, which the
migration rules accept:

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE user_names (user_id BIGINT PRIMARY KEY REFERENCES users (id), name TEXT NOT NULL);
```

```sql migration
-- error:
BEGIN;
SET LOCAL lock_timeout = '400ms';
SET LOCAL statement_timeout = '60s';
INSERT INTO user_names (user_id, name)
SELECT id, name FROM users WHERE id > 5000 ORDER BY id LIMIT 500
ON CONFLICT (user_id) DO UPDATE SET name = excluded.name
WHERE user_names.name IS DISTINCT FROM excluded.name;
COMMIT;
```

Before you drop anything:

- **Remove the last reader first.** Writes that keep the column consistent may
  stay. Drop it once no deployed version reads it, self-hosted releases
  included: check `--against` a directory with one snapshot per release.
- **Replace a foreign key's `ON DELETE` action** before you drop the column or
  constraint that carries it; `dropped-delete-action` reports the drop.
- **Keep a SQL function that the previous release calls**, and drop it in the
  next one. `sqlty breaking` reports each call to a dropped function as
  `unknown-function`, a warning by default.
- **Prove a code path is dead before you delete it**, for example by finding no
  calls in production telemetry over a full release cycle. Drop its table in a
  later migration.
- **Don't backfill rows you can't identify.** Repair such a row the next time the
  code reads or writes it.
- **Count the rows a new constraint or rule rejects** with a read-only query
  before you deploy it.

Locks across releases:

- Keep `lock_timeout` below `deadlock_timeout` (1 s by default) when a migration
  takes several locks, and retry on timeout. `missing-lock-timeout` checks that it
  is set, not its value ([#1259](https://github.com/jirikuncar/sqlty/issues/1259)).
- `ADD CONSTRAINT ... NOT VALID` and `VALIDATE CONSTRAINT` go in separate
  transactions, best in separate releases. In one transaction the `ADD` lock is
  held through the scan, and `constraint-scan` reports the `VALIDATE`.
- Until a migration ships, edit it, rather than add a second one that locks the
  same busy table again.
- An index build that can outlast the deploy runs after it, can be rerun (drop
  an invalid leftover first), and nothing depends on it.

## In CI

The base branch must be in the clone:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- run: uv run sqlty breaking --output-format github --against origin/${{ github.base_ref }}
```

See `sqlty-ci` for the whole job.

When the baseline is not in git history, save a snapshot at release time and
check against it, or against a directory holding one snapshot per version still
deployed. A schema `.sql` file (a dump) is a baseline for the forward check only:

```bash
sqlty breaking --save .sqlty/releases/v1.4.json
sqlty breaking --against .sqlty/releases/
sqlty breaking --against dump.sql
```

## Reference

- [Breaking schema changes](https://sqlty.kuncar.dev/guide/breaking-changes/)
- [Schema changes in CI](https://sqlty.kuncar.dev/recipes/migration/#schema-changes-in-ci)

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
| backward `stale-type`, `nullable-mismatch` | a column became nullable under code that expects a value | widen the code to `T \| None` first |
| forward `unknown-column`, `unknown-table` | new code reads what the old schema doesn't have yet | add the schema in an earlier pull request |

Only what the other schema breaks is reported: a diagnostic a query already gets
against its own schema is not repeated, and neither is `stale-rewrite` or
`unused-ignore`.

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

## In CI

The base branch must be in the clone:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- run: uv run sqlty breaking --output-format github --against origin/${{ github.base_ref }}
```

See `sqlty-ci` for the whole job.

## Reference

- [Breaking schema changes](https://jirikuncar.github.io/sqlty/guide/breaking-changes/)
- [Schema changes in CI](https://jirikuncar.github.io/sqlty/recipes/migration/#schema-changes-in-ci)

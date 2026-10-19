# Breaking schema changes

```bash
sqlty breaking --against origin/main
```

During a rolling deploy, the code of one revision runs against the schema of
another. `sqlty breaking` checks both mixes before you deploy:

| Direction | Checks | Happens when you |
|-----------|--------|------------------|
| backward | the queries of `--against` on the schema of `--head` | migrate first: old code, new schema |
| forward | the queries of `--head` on the schema of `--against` | deploy first: new code, old schema |

`--against` is the baseline revision: a branch, tag, SHA or `HEAD~1`, or a
file ([Baselines without git history](#baselines-without-git-history)). `--head`
is the new one, the working tree by default (uncommitted changes included).
`--direction backward` or `forward` runs one check; both run by default.

Each revision is read as `sqlty check` reads it: its own `pyproject.toml`,
`schema` and Python files. A revision other than the working tree is checked out
into a temporary `git worktree`, removed afterwards, so your working tree is
never touched.

`sqlty breaking` exits 0 when both mixes are compatible, 1 when a change breaks
one of them with an `error` diagnostic, and 2 on an error such as an unknown
revision. A `warn` finding is printed but does not fail the run. `--output-format json`
prints the findings as a JSON array, each with its `direction` and the baseline it
was checked `against`, and `github` as GitHub Actions annotations.

## What it reports

Only what the other schema breaks: a diagnostic a query already gets against its
own schema is not repeated. The rules and severities are the ones `sqlty check`
uses. By default `unknown-table`, `unknown-column`, `ambiguous-column` and
[`insert-missing-column`](../reference/rules.md#insert-missing-column) are errors,
and `unknown-function` and `unknown-type` warnings, so a name that no longer
resolves is reported wherever the query reads it: the select list, `WHERE`,
`JOIN ... ON`, and what `INSERT`, `UPDATE` and `DELETE` write. A rule you set to
`ignore`, for all files or in an override, stays off: a function of an extension
the schema does not declare is no breaking change. A row type annotation that no longer fits is reported by the usual
[row type checks](row-types.md).

The examples below check code written for `schema.sql` against `against.sql`.

## Backward: old code, new schema

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, bio TEXT, nick TEXT NOT NULL DEFAULT '');
CREATE TABLE sessions (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL);
```

### A dropped table or column

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, nick TEXT NOT NULL DEFAULT '');
```

```python
from sqlty import SQL

# error: [unknown-column] "column `bio` does not exist in `users`"
bios = SQL[tuple[str | None]]("--sql\nSELECT bio FROM users")
# error: [unknown-table] "table `sessions` is not defined in the schema"
purge = "--sql\nDELETE FROM sessions WHERE user_id = %s"
# error: [unknown-column] "column `bio` does not exist in `users`"
clear = "--sql\nUPDATE users SET nick = '' WHERE bio IS NULL"
```

### A renamed column

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email_address TEXT NOT NULL, bio TEXT, nick TEXT NOT NULL DEFAULT '');
CREATE TABLE sessions (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL);
```

```python
from sqlty import SQL

# error: [unknown-column] "column `email` does not exist in `users`"
emails = SQL[tuple[str]]("--sql\nSELECT email FROM users")
```

### A new NOT NULL column without a default

An `INSERT` written before the column existed does not set it:

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, bio TEXT, nick TEXT NOT NULL DEFAULT '', name TEXT NOT NULL);
CREATE TABLE sessions (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL);
```

```python
# error: [insert-missing-column] "INSERT into `users` leaves out `name`, which is NOT NULL and has no default"
signup = "--sql\nINSERT INTO users (email) VALUES (%s)"
```

### A column that may now be NULL

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT, bio TEXT, nick TEXT NOT NULL DEFAULT '');
CREATE TABLE sessions (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL);
```

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[str | None]]"
emails = SQL[tuple[str]]("--sql\nSELECT email FROM users")
```

## Forward: new code, old schema

`schema.sql` is now the new schema, and `against.sql` the old one.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, handle TEXT);
CREATE TABLE teams (id SERIAL PRIMARY KEY, title TEXT NOT NULL);
```

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL);
```

A query that reads a column or table the old schema doesn't have yet fails until
the migration runs:

```python
from sqlty import SQL

# error: [unknown-column] "column `handle` does not exist in `users`"
handles = SQL[tuple[str | None]]("--sql\nSELECT handle FROM users")
# error: [unknown-table] "table `teams` is not defined in the schema"
teams = SQL[tuple[str]]("--sql\nSELECT title FROM teams")
# error: [unknown-column] "column `handle` does not exist in `users`"
claim = "--sql\nUPDATE users SET handle = %s WHERE id = %s"
```

A query that works with both schemas is not reported:

```python
from sqlty import SQL

# error:
emails = SQL[tuple[str]]("--sql\nSELECT email FROM users")
```

### Rules about the code

[`stale-rewrite`](../reference/rules.md#stale-rewrite) and
[`unused-ignore`](../reference/rules.md#unused-ignore) judge the code, not the
schema change, so they are not reported. Below, the old schema can type the marked
string and leaves the suppression unused, and neither is a breaking change.

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL);
```

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, bio TEXT);
```

```python
# error:
bios = "--sql\nSELECT bio FROM users"  # sqlty: ignore[unknown-column]
```

### A column that may now be NULL

Code annotated `str | None` for a column the new schema makes nullable reads the
`str` rows of the old schema too. An annotation wider than the other schema's rows
is not reported, though `sqlty check` against the old schema would report it as
[`stale-type`](../reference/rules.md#stale-type).

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT);
```

`against.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL);
```

```python
from sqlty import SQL

# error:
emails = SQL[tuple[str | None]]("--sql\nSELECT email FROM users")
```

## Baselines without git history

```bash
# At release time
sqlty breaking --save .sqlty/releases/v1.4.json

# In every pull request
sqlty breaking --against .sqlty/releases/
```

`--save PATH` writes a snapshot of `--head`: its Python files, the settings each
is checked with, and its schema. `--against` reads a snapshot as it reads the
revision it was saved from, so the release stays a baseline after its history is
rewritten, in a shallow clone, or when it was never a commit.

`--against` takes a path that exists instead of a revision:

| Baseline | Backward | Forward |
|----------|----------|---------|
| a git revision | yes | yes |
| a snapshot (`.json`) | yes | yes |
| a directory of snapshots | each snapshot | each snapshot |
| a schema `.sql` file, or a directory of them | no, it has no queries | yes |

A directory of snapshots checks every version in it, named after its file. Keep a
snapshot for each version that may still run, during a rolling or on-premises
deploy or a rollback window, and remove it when that version is gone:

```text
app/q.py:2: [unknown-column] column `bio` does not exist in `users` (backward: the queries of v1.3 against the schema of the working tree)
```

A schema file, such as the output of `pg_dump --schema-only` in CI, has no
queries, so it is only checked forward: the new queries against that schema.

!!! note "A snapshot holds source code"
    A snapshot holds the full text of every checked Python file. Commit it where the
    code may be read, or store it as a CI artifact.

## Expand and contract

To change a schema without breaking either mix, split the change into pull
requests that each pass `sqlty check` and `sqlty breaking` in both directions.
Renaming `users.name` to `display_name`:

| Pull request | Schema | Code |
|--------------|--------|------|
| 1. Widen the code | unchanged | annotate `name` as `str \| None`, with `# sqlty: ignore[stale-type]` |
| 2. Expand the schema | add `display_name`, nullable or with a `DEFAULT`; relax `name` to nullable | drop the `ignore` |
| 3. Expand the code | unchanged | write both columns, read `display_name` |
| 4. Migrate the data | backfill `display_name` | unchanged |
| 5. Contract the code | unchanged | read and write only `display_name` |
| 6. Contract the schema | drop `name`; make `display_name` `NOT NULL` | `sqlty check --fix` narrows `display_name` to `str` |

The order matters:

- **Widen the code before the schema.** Relaxing `NOT NULL` makes the old code's
  `SQL[tuple[str]]` wrong for the new schema, which the backward check reports. Code
  annotated `str | None` reads the rows of both schemas, so ship it first. Against
  the old schema, `stale-type` says the annotation could be `str`: suppress it on
  that line until the schema changes, then remove the suppression, which
  [`unused-ignore`](../reference/rules.md#unused-ignore) reminds you of. `sqlty breaking`
  doesn't report an annotation wider than the other schema's rows.
- **Schema before the code that uses it.** Code that writes `display_name` in the
  same pull request that adds it fails the forward check: the new code runs against
  the old schema if it deploys first. If your migrations always run before the
  deploy, `--direction backward` checks only that mix and accepts them together.
- **Contract the code before the schema.** An `INSERT` that leaves out a column
  which is still `NOT NULL` fails at runtime. Turn on
  [`insert-missing-column`](../reference/rules.md#insert-missing-column) so
  `sqlty check` reports it against the pull request's own schema.

[Migration](../recipes/migration.md#schema-changes-in-ci) shows how to run
`sqlty breaking` in CI.

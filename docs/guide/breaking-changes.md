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

`--against` is the baseline revision: a branch, tag, SHA or `HEAD~1`. `--head`
is the new one, the working tree by default (uncommitted changes included).
`--direction backward` or `forward` runs one check; both run by default.

Each revision is read as `sqlty check` reads it: its own `pyproject.toml`,
`schema` and Python files. A revision other than the working tree is checked out
into a temporary `git worktree`, removed afterwards, so your working tree is
never touched.

`sqlty breaking` exits 0 when both mixes are compatible, 1 when a change breaks
one of them, and 2 on an error such as an unknown revision.

## What it reports

Only what the other schema breaks: a diagnostic a query already gets against its
own schema is not repeated. On top of the rules you configure, it turns on
`unknown-table`, `unknown-column`, `ambiguous-column`, `unknown-function`,
`unknown-type` and [`insert-missing-column`](../reference/rules.md#insert-missing-column),
so a name that no longer resolves is reported wherever the query reads it: the
select list, `WHERE`, `JOIN ... ON`, and what `INSERT`, `UPDATE` and `DELETE`
write. A row type annotation that no longer fits is reported by the usual
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

## Expand and contract

To change a schema without breaking either mix, split the change:

1. Expand: add the new table or column, nullable or with a `DEFAULT`, and
   deploy code that writes both.
2. Migrate the data.
3. Contract: deploy code that reads only the new shape, then drop the old one.

`sqlty breaking` passes at every step of that sequence. [Migration](../recipes/migration.md#schema-changes-in-ci)
shows how to run it in CI.

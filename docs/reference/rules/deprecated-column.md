---
order: 195
group: "names"
severity: "warn"
fixable: false
summary: "Query reads a column scheduled for removal"
origin: "SQL"
---

# deprecated-column

Default severity: **warn**

## What it does

Reports each read of a column that
[`deprecated-columns`](../configuration.md#deprecated-columns) marks: in the select
list, `WHERE`, `JOIN ... ON`, `GROUP BY`, `ORDER BY`, `RETURNING`, the arguments of
`CASE`, `COALESCE` and other functions, and the values of `INSERT` and `UPDATE`.
Writing it, in an `INSERT` column list or as the target of `UPDATE ... SET`, is
not reported.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.deprecated-columns]
"users.legacy_email" = { use = "email", remove-after = "2027-02-20" }
```

`schema.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT, legacy_email TEXT);
```

```python
from sqlty import SQL

# error: [deprecated-column] "`users.legacy_email` is deprecated: read `email` instead, it is removed after 2027-02-20"
emails = SQL[tuple[str | None]]("--sql\nSELECT COALESCE(email, legacy_email) FROM users")
```

```python
from sqlty import SQL

# error:
sync = SQL("--sql\nUPDATE users SET email = %s, legacy_email = %s WHERE id = %s")
```

## Why is this bad?

A safe column removal takes several releases: stop reading the column, keep
writing it while older releases still read it, then drop it
([GitLab](https://docs.gitlab.com/development/database/avoiding_downtime_in_migrations/),
[Fowler](https://www.martinfowler.com/bliki/ParallelChange.html)). A read added
in between, often a fallback after a backfill already made the column dead,
breaks once the drop ships. [`sqlty breaking`](../../guide/breaking-changes.md)
reports it only then, when the migration that drops the column lands.

## How to fix

Read the replacement column. Keep writing the deprecated one until the release
that drops it.

## Configuration

Mark a column in `[tool.sqlty.deprecated-columns]`, keyed by `table.column`.
`use` names the replacement and `remove-after` the date, both optional and shown
in the message. A database of
[`databases`](../configuration.md#databases) takes its own table.

```toml
[tool.sqlty.rules]
deprecated-column = "error"
```

`SELECT *` reads every column without naming one, so it is not reported here;
[`select-star`](select-star.md) asks for the columns by name.

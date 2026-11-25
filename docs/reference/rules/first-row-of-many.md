---
order: 175
group: "query"
severity: "warn"
fixable: false
summary: "Call reads only the first row of a query that can return many"
origin: "Python"
---

# first-row-of-many

Default severity: **warn**

## What it does

Reports a call that reads only the first row of a query when the query can return
more than one: `fetchone()` of a psycopg, sqlite3 or aiosqlite `execute(...)`,
asyncpg's `fetchrow` and `fetchval`, and the `scalar` helper of
`sqlty.drivers.psycopg`.

A query returns at most one row when it aggregates without `GROUP BY`, has
`LIMIT 1` or `FETCH FIRST ROW ONLY`, inserts one row, or when its conditions fix
every column of a primary key, `UNIQUE` constraint or unique index of each table
it reads, to a constant, a parameter or a column of a table already fixed. An
`OR`, `IN` or range condition fixes nothing. A query that reads a table whose keys
are unknown, a CTE, or a set operation is not reported.

## Why is this bad?

The rows after the first are dropped without a word. Which row comes first is up
to the database unless the query orders them, so the result can change from one
run to the next.

## Example

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, name TEXT NOT NULL, active BOOLEAN NOT NULL);
```

```python
import sqlite3

conn = sqlite3.connect("app.db")
# error: [first-row-of-many] "fetchone() reads the first row of a query that can return many"
conn.execute("SELECT name FROM users WHERE active").fetchone()
conn.execute("SELECT name FROM users WHERE email = ?", ("a@b.c",)).fetchone()
conn.execute("SELECT count(*) FROM users").fetchone()
```

## Fix

Fix a key in `WHERE`, or say which row you want with `ORDER BY ... LIMIT 1`.
Turn the rule off with `first-row-of-many = "ignore"` in `[tool.sqlty.rules]`.

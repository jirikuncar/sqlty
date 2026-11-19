+++
order = 380
group = "names"
severity = "error"
fixable = false
summary = "Query the database rejects for a reason no other rule names"
origin = "SQL"
reported-by = ["lexer", "names", "query-files"]
+++

# rejected-query

Default severity: **error**

## What it does

Reports a query the database rejects for a reason no other rule names: a
`CASE` or `COALESCE` whose results have no common type, a `SELECT *` with no
`FROM`, a SQLite hex literal that does not fit 64 bits, and `EXTRACT` in SQLite.

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "postgres"

[tool.sqlty.rules]
stale-rewrite = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE events (id INTEGER PRIMARY KEY, day DATE NOT NULL, note TEXT);
```

```python
from sqlty import SQL

# error: [rejected-query] "CASE types INT and DATE cannot be matched"
q = SQL("--sql\nSELECT CASE WHEN id > 1 THEN 1 ELSE day END FROM events")
# error: [rejected-query] "argument types TEXT and INT cannot be matched"
r = SQL("--sql\nSELECT coalesce(note, id) FROM events")
# error: [rejected-query] "`SELECT *` with no FROM selects no columns"
s = SQL("--sql\nSELECT *")
```

A string literal or a parameter takes the type of the other results, so it
always matches:

```python
import datetime

from sqlty import SQL

# error:
q = SQL[tuple[datetime.date]]("--sql\nSELECT coalesce(day, '2024-01-01') FROM events")
r = SQL[tuple[str | None]]("--sql\nSELECT CASE WHEN id > %s THEN note ELSE %s END FROM events")
```

## Why is this bad?

PostgreSQL rejects the query: `CASE types integer and date cannot be matched`,
`SELECT * with no tables specified is not valid`. SQLite says `no tables
specified`, `hex literal too big` and, for `EXTRACT`, `near "FROM": syntax
error`. MySQL `No tables used`, DuckDB
`* expression without FROM clause`. The column has no type to infer.

## How to fix

Cast the results to one type (`day::text`), select the columns by name, or
spell the number in decimal. In SQLite, read a date part with `strftime`:
`strftime('%Y', day)`.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
rejected-query = "warn"
```

Needs no schema. Results of different types are judged in the `postgres`
dialect, by PostgreSQL's type categories: numbers, strings, dates and times,
booleans and intervals. `SELECT *` with no `FROM` is judged in the
`postgres`, `mysql`, `sqlite` and `duckdb` dialects; ClickHouse reads its
`system.one` table.

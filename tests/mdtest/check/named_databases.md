# Named databases

A query names its database on its marker line, `--sql sqlty:db=NAME`; it is read against
that database's dialect and schema.

```toml
[tool.sqlty]
schema = "app.sql"

[tool.sqlty.databases.analytics]
dialect = "sqlite"
schema = "analytics.sql"

[tool.sqlty.databases.cache]
dialect = "mysql"
```

`app.sql`:

```sql
CREATE TABLE users (id TEXT NOT NULL);
```

`analytics.sql`:

```sql
CREATE TABLE events (n INTEGER NOT NULL, name TEXT);
```

## Rewrite

```python
users = "--sql\nSELECT id FROM users"
events = "--sql sqlty:db=analytics\nSELECT n FROM events"
default = "--sql sqlty:db=default\nSELECT id FROM users"
cached = "-- sql sqlty:db=cache\nSELECT 1 AS one"
# rewritten: users = SQL[tuple[str]]("--sql\nSELECT id FROM users")
# rewritten: events = SQL[tuple[int]]("--sql sqlty:db=analytics\nSELECT n FROM events")
# rewritten: default = SQL[tuple[str]]("--sql sqlty:db=default\nSELECT id FROM users")
# rewritten: cached = SQL[tuple[int]]("-- sql sqlty:db=cache\nSELECT 1 AS one")
```

## Row types are checked against the named database

```python
from sqlty import SQL

# error:
q = SQL[tuple[int]]("--sql sqlty:db=analytics\nSELECT n FROM events")
# error:
r = SQL[tuple[str]]("--sql\nSELECT id FROM users")
# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
s = SQL[tuple[str]]("--sql sqlty:db=analytics\nSELECT n FROM events")
# error: [unknown-table] "table `users` is not defined in the schema"
t = SQL[tuple[int]]("--sql sqlty:db=analytics\nSELECT id FROM users")
```

## A database without a schema reads queries without one

```python
from sqlty import SQL

# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("-- sql sqlty:db=cache\nSELECT * FROM sessions")
```

## `SELECT *` expands from the named database

```python
from sqlty import SQL

q = SQL[tuple[int, str | None]]("--sql sqlty:db=analytics\nSELECT * FROM events")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str | None]]("--sql sqlty:db=analytics\nSELECT n, name FROM events")
```

## Unknown database

```python
from sqlty import SQL

# error: [unknown-database] "database `analytic` is not configured; did you mean `analytics`?"
q = SQL[tuple[int]]("--sql sqlty:db=analytic\nSELECT 1::INTEGER")
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
# error: [unknown-database] "database `warehouse` is not configured"
r = "--sql sqlty:db=warehouse\nSELECT 1::INTEGER"
```

## Helpers of a database

A helper listed in a database's `functions` checks its calls against that database, in
any file. A query that names its database keeps it.

```toml
[[tool.sqlty.databases.analytics.functions]]
import-path = "pkg.warehouse.fetch"
query-param = "query"
return-type-param = "form"
```

`pkg/warehouse.py`:

```python
def fetch[T](query: str, form: type[T]) -> list[T]: ...
```

```python
from pkg.warehouse import fetch

fetch("SELECT n FROM events", tuple[int])
fetch("--sql sqlty:db=default\nSELECT id FROM users", tuple[str])
# error: [column-type-mismatch] "tuple[str] item 0 is str but query column 'n' infers int"
fetch("SELECT n FROM events", tuple[str])
# error: [unknown-table] "table `users` is not defined in the schema"
fetch("SELECT id FROM users", tuple[str])
```

## Query files

A `.sql` file names its database in a header line above its first query.

```sql
-- sqlty: db=analytics

-- name: count_events
-- error:
SELECT n FROM events;

-- name: users
-- error: [unknown-table] "table `users` is not defined in the schema"
SELECT id FROM users;
```

```sql
-- error: [unknown-database] "database `analytcs` is not configured; did you mean `analytics`?"
-- sqlty: db=analytcs

-- name: count_events
-- error: [unknown-table] "table `events` is not defined in the schema"
SELECT n FROM events;
```

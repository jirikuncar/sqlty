---
name: sqlty-adopt
description: Introduce sqlty into an existing Python project with raw SQL queries, one module at a time. Use when a project doesn't use sqlty yet and you are asked to add it, type its SQL queries, configure [tool.sqlty] (dialect, schema from a database dump or migrations), choose which queries to convert first, mark the ones sqlty can't type yet, or add the first CI gate.
---

# Adopt sqlty

sqlty only acts on strings that start with `--sql`, so you can adopt it one
module, or one query, at a time. Unconverted code keeps working.

Run `sqlty version`: this skill may be newer or older than the installed sqlty.
When it and `sqlty --help` disagree, trust the command.

## 1. Install and configure

```bash
uv add sqlty            # or: pip install sqlty; sqlty requires Python >= 3.14
```

Find the dialect from the driver (`psycopg` or `asyncpg`: `postgres`;
`sqlite3`: `sqlite`; `pymysql` or `mysqlclient`: `mysql`) and the schema. A
schema saves a cast in every query. Dump it from the database, or point at
the migrations:

```bash
pg_dump --schema-only app > schema.sql      # PostgreSQL
sqlite3 app.db .schema > schema.sql         # SQLite
mysqldump --no-data app > schema.sql        # MySQL
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"   # or a directory of migrations: "migrations/"
```

`schema.sql`:

```sql
CREATE TABLE public.users (
    id integer NOT NULL,
    name text NOT NULL,
    created_at timestamp with time zone DEFAULT now()
);
```

Statements that don't shape a table, such as grants and indexes, are ignored,
and a schema-qualified table matches an unqualified query:

```python
query = "--sql\nSELECT id, name, created_at FROM users WHERE id = %s"
# rewritten: query = SQL[tuple[int, str, datetime.datetime | None]]("--sql\nSELECT id, name, created_at FROM users WHERE id = %s")
```

## 2. Convert one module

Pick a module whose queries are plain string literals. Add `--sql` and a
newline to the start of each query, then:

```bash
sqlty rewrite app/users.py
sqlty check app/users.py
```

Skip queries built with f-strings, `+` or `%`: convert them to parameters
(`%s`, `$1`, `?`) first, in a separate change. A `--sql` string that is still
unwrapped after `rewrite` has a column sqlty can't type: cast it, or add its
table to the schema.

`sqlty check` lists the `--sql` strings left unwrapped as `stale-rewrite`, so
you can see what is left. To make each untyped query visible as
`SQL[UNTYPED]`, set `untyped = "sentinel"`.

## 3. Name the important rows

Replace `tuple[...]` with a `TypedDict`, `dataclass` or `NamedTuple` where a
query feeds domain code, starting with the rows passed around the most. With
psycopg, `sqlty.drivers.psycopg.execute` returns rows of that type:

```python
from dataclasses import dataclass
from datetime import datetime

from sqlty import SQL
from sqlty.drivers.psycopg import execute

@dataclass
class User:
    id: int
    name: str
    created_at: datetime

GET_USER = SQL[User]("--sql\nSELECT id, name, created_at FROM users WHERE id = %s")

def get_user(conn, user_id: int) -> User | None:
    return execute(conn, GET_USER, (user_id,)).fetchone()
```

`sqlty check` now fails if `users` changes and `User` doesn't follow.

## 4. Gate it in CI

Start with what passes today, then tighten:

```yaml
- run: uv run sqlty rewrite --check .
- run: uv run sqlty check --output-format github .
```

Promote `stale-type` to `error` once the converted modules are clean (by default
it only warns when an annotation is wider than the rows), and add
`sqlty breaking` when schema changes start going through pull requests. See
`sqlty-ci`.

## Don't

- Don't convert every module in one change: each module is reviewable on its own.
- Don't hand-write `T`, and don't strip a file with named row types.
- Don't write `"--sql SELECT ..."` with a space: the query is a SQL comment.

## Reference

- [Migration](https://jirikuncar.github.io/sqlty/recipes/migration/)
- [Schema inference](https://jirikuncar.github.io/sqlty/guide/schema-inference/)
- [Drivers and ORMs](https://jirikuncar.github.io/sqlty/guide/drivers-and-orms/)
- [Configuration](https://jirikuncar.github.io/sqlty/reference/configuration/)

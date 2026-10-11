# MySQL `--sql` Marker

MySQL only reads `--` followed by whitespace as a comment. sqlty drops the
`--sql` marker line before parsing, so these queries are typed in MySQL too.

The marker is still part of the query: `SQL[T]` is a `str`, and the driver
sends it to the server unchanged. MySQL and MariaDB servers reject a query
that starts with `--sql` followed by a newline (error 1064), so
`mysql-sql-marker` reports it, and `-- sql` is the marker to use.

## Rewrite

The space form `--sql SELECT ...` is a comment in every dialect, with nothing
after it to parse, so `c` stays unwrapped.

`schema.sql`:
```sql
CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```python
a = "--sql\nSELECT id, name FROM users"
b = """--sql
SELECT id, UPPER(name) AS shout FROM users WHERE name LIKE 'a%%'"""
c = "--sql SELECT id FROM users"
# rewritten: a = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users")
# rewritten: b = SQL[tuple[int, str]]("""--sql
# rewritten: SELECT id, UPPER(name) AS shout FROM users WHERE name LIKE 'a%%'""")
# rewritten: c = "--sql SELECT id FROM users"
```

## Stale type

`schema.sql`:
```sql
CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```python
# error: [mysql-sql-marker]
# error: [stale-type] "SQL[tuple[str, str]] should be SQL[tuple[int, str]]"
a = SQL[tuple[str, str]]("--sql\nSELECT id, name FROM users")
```

## The `-- sql` marker

`-- sql` followed by a line break is a comment in every dialect, and sqlty reads
it as the marker: it wraps, strips and checks these strings as it does `--sql`.

`schema.sql`:
```sql
CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```python
a = "-- sql\nSELECT id, name FROM users"
b = """-- sql
SELECT id FROM users"""
c = "-- sql query notes"
# rewritten: a = SQL[tuple[int, str]]("-- sql\nSELECT id, name FROM users")
# rewritten: b = SQL[tuple[int]]("""-- sql
# rewritten: SELECT id FROM users""")
# rewritten: c = "-- sql query notes"
```

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str, str]] should be SQL[tuple[int, str]]"
a = SQL[tuple[str, str]]("-- sql\nSELECT id, name FROM users")
```

```python
from sqlty import SQL

a = SQL[tuple[int, str]]("-- sql\nSELECT id, name FROM users")
# stripped: a = "-- sql\nSELECT id, name FROM users"
```

## Fix

`sqlty check --fix` turns `--sql` into `-- sql` in plain string literals, the
only change it makes to string content besides `inline-sql-comment` and
`select-star`.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
a = "--sql\nSELECT CAST(1 AS SIGNED) AS n"
b = """--sql
SELECT CAST(1 AS SIGNED) AS n"""
c = f"--sql\nSELECT {a}"
# fixed: from sqlty import SQL
# fixed: a = SQL[tuple[int]]("-- sql\nSELECT CAST(1 AS SIGNED) AS n")
# fixed: b = SQL[tuple[int]]("""-- sql
# fixed: SELECT CAST(1 AS SIGNED) AS n""")
# fixed: c = f"--sql\nSELECT {a}"
```

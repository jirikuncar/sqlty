# MySQL `--sql` Marker

MySQL only reads `--` followed by whitespace as a comment. sqlty drops the
`--sql` marker line before parsing, so these queries are typed in MySQL too.

The marker is still part of the query: `SQL[T]` is a `str`, and the driver
sends it to the server unchanged. MySQL and MariaDB servers reject a query
that starts with `--sql` followed by a newline (error 1064), so drop the
marker line before executing it.

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
# error: [stale-type] "SQL[tuple[str, str]] should be SQL[tuple[int, str]]"
a = SQL[tuple[str, str]]("--sql\nSELECT id, name FROM users")
```

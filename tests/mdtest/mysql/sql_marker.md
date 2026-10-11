# MySQL `--sql` Marker

MySQL only reads `--` followed by whitespace as a comment, but the `--sql`
marker line is sqlty's own syntax and is never part of the query.

## Rewrite

`schema.sql`:
```sql
CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```python
a = "--sql\nSELECT id, name FROM users"
b = """--sql
SELECT id, UPPER(name) AS shout FROM users WHERE name LIKE 'a%'"""
c = "--sql SELECT id FROM users"
# rewritten: a = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users")
# rewritten: b = SQL[tuple[int, str]]("""--sql
# rewritten: SELECT id, UPPER(name) AS shout FROM users WHERE name LIKE 'a%'""")
# rewritten: c = "--sql SELECT id FROM users"
```

## Stale type

`schema.sql`:
```sql
CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```python
from sqlty import SQL

a = SQL[tuple[str, str]]("--sql\nSELECT id, name FROM users")
# check: [stale-type] "SQL[tuple[str, str]] should be SQL[tuple[int, str]]"
```

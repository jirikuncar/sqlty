# SELECT * Checks

## Star with no schema is an error

```python
from sqlty import SQL
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
```

## Star with no schema reports no stale type

Without a schema the columns of `*` are unknown, so inference gives no type
and the annotation is not compared with a guess.

```python
from sqlty import SQL, UNTYPED
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[int]]("--sql\nSELECT * FROM users")
r = SQL[tuple[int, int]]("--sql\nSELECT id, * FROM users")
s = SQL[tuple[int]]("--sql\nSELECT u.* FROM users u")
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
t = SQL[UNTYPED]("--sql\nSELECT id::INTEGER, * FROM users")
```

## Star with schema produces warning

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
from sqlty import SQL
# error: [select-star] "SELECT * used — expand to explicit column list (joins need a manual edit)"
# error: [stale-type] "SQL[tuple[int, str, str]] should be SQL[tuple[int, str, str | None]]"
q = SQL[tuple[int, str, str]]("--sql\nSELECT * FROM users")
```

## Star with schema and rule disabled produces no error

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
[tool.sqlty.rules]
select-star = "ignore"
```

```python
from sqlty import SQL
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# error:
```

## Explicit columns produce no error

```python
from sqlty import SQL
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

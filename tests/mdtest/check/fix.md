# Fixes (`sqlty check --fix`)

`# fixed:` shows the source `sqlty check --fix` writes, imports included.

## Stale tuple type is updated

```python
from sqlty import SQL
q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Unwrapped string is wrapped

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Inline --sql comment gets a newline and is wrapped

```python
q = "--sql SELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## New datetime type adds its import

```python
from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT created_at::TIMESTAMP FROM users")
# fixed: import datetime
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP FROM users")
```

## Multiline wrapper keeps its layout

```python
from sqlty import SQL
q = SQL[tuple[str]](
    """--sql
SELECT id::INTEGER
FROM users"""
)
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int]](
# fixed:     """--sql
# fixed: SELECT id::INTEGER
# fixed: FROM users"""
# fixed: )
```

## SELECT star is expanded with a schema

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL
a = SQL[tuple[int, str]]("--sql\nSELECT * FROM users\nWHERE id = 1")
b = SQL[tuple[int, str]]("""--sql
SELECT u.* FROM users u""")
# fixed: from sqlty import SQL
# fixed: a = SQL[tuple[int, str]]("--sql\nSELECT id::INT, name::TEXT FROM users\nWHERE id = 1")
# fixed: b = SQL[tuple[int, str]]("""--sql
# fixed: SELECT id::INT, name::TEXT FROM users u""")
```

## SELECT star is kept outside postgres

The expansion uses `::` casts, which SQLite and MySQL lack.

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

```python
from sqlty import SQL
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
```

## Ignored rules are not fixed

```toml
[sqlty]
[sqlty.rules]
stale-type = "ignore"
stale-rewrite = "ignore"
inline-sql-comment = "ignore"
```

```python
from sqlty import SQL
a = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
b = "--sql\nSELECT id::INTEGER FROM users"
c = "--sql SELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed: a = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: b = "--sql\nSELECT id::INTEGER FROM users"
# fixed: c = "--sql SELECT id::INTEGER FROM users"
```

## Row types, f-strings and t-strings are left alone

```python
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    id: str

table = "users"
a = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
b = f"--sql SELECT id::INTEGER FROM {table}"
c = t"--sql\nSELECT id::INTEGER FROM {table}"
# fixed: from typing import NamedTuple
# fixed: from sqlty import SQL
# fixed:
# fixed: class User(NamedTuple):
# fixed:     id: str
# fixed:
# fixed: table = "users"
# fixed: a = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# fixed: b = f"--sql SELECT id::INTEGER FROM {table}"
# fixed: c = t"--sql\nSELECT id::INTEGER FROM {table}"
```

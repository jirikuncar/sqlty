# Unknown column type is written as str

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW(),
    location GEOMETRY
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Unknown function

```sql
SELECT some_extension_function(id) AS x FROM users
-- inferred: None
```

```python
query = "--sql\nSELECT some_extension_function(id) AS x FROM users"
# rewritten: query = "--sql\nSELECT some_extension_function(id) AS x FROM users"
```

## One unknown column leaves the whole row unknown

```sql
SELECT id, some_extension_function(id) AS x, name FROM users
-- inferred: None
-- columns: id: int (INT)
-- columns: x: None (None)
-- columns: name: str (TEXT)
```

## Unknown schema and cast types

```sql
SELECT id, location FROM users
-- inferred: None
```

```sql
SELECT id, name::CUSTOM_TYPE FROM users
-- inferred: None
```

## lower and upper only type strings

A range (or any other non-string) argument has no `str` result.

```sql
SELECT lower(name), upper(email) FROM users
-- inferred: tuple[str, str | None]
```

```sql
SELECT lower(location) FROM users
-- inferred: None
```

## The sentinel marks the string

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
untyped = "sentinel"
```

```python
query = "--sql\nSELECT some_extension_function(id) AS x FROM users"
# rewritten: query = SQL[UNTYPED]("--sql\nSELECT some_extension_function(id) AS x FROM users")
```

## Checks skip unknown columns

```python
from sqlty import SQL

# error: [unknown-function] "function `some_extension_function` is not known to sqlty, so its result is untyped"
q = SQL[tuple[int, bytes]]("--sql\nSELECT id, some_extension_function(id) FROM users")
```

```python
from typing import NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    id: int
    x: dict

# error: [unknown-function] "function `some_extension_function` is not known to sqlty, so its result is untyped"
q = SQL[Row]("--sql\nSELECT id, some_extension_function(id) AS x FROM users")
```

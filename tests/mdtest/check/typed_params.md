# Typed parameters

`SQL[R, P]`: `check` reads `R` as the row type and ignores `P`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
```

## Row type with positional parameters

```python
from sqlty import SQL

q = SQL[tuple[str], tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
# error:
```

## Row type with named parameters

```python
from typing import TypedDict

from sqlty import SQL


class ById(TypedDict):
    id: int


class User(TypedDict):
    id: str


# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User, ById]("--sql\nSELECT id, name FROM users WHERE id = %(id)s")
```

## A stale row type is fixed, its parameters kept

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
q = SQL[tuple[int], tuple[()]]("--sql\nSELECT name FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[str], tuple[()]]("--sql\nSELECT name FROM users")
```

## A multiline subscript

```python
from sqlty import SQL

q = SQL[
    tuple[str],
    tuple[int],
]("--sql\nSELECT name FROM users WHERE id = %s")
# error:
```

# Query variant paths

Paths that bind the same names are merged only when the merged conditions take exactly
those paths, so no variant combines conditions that no input can.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE foos (id INTEGER PRIMARY KEY, n INTEGER NOT NULL);
```

`db.py`:

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...
```

## A return in a nested branch keeps the conditions of the other paths

`b and not c` always adds `AS k`, so no input runs `SELECT id FROM foos ORDER BY k`.

```python
from typing import LiteralString

from db import fetch_all


def find(a: bool, b: bool, c: bool) -> list[tuple[int]]:
    query: LiteralString = "SELECT id"
    if b:
        if c:
            if a:
                return []
        else:
            query += " AS k"
    elif a:
        query += " + 0"
    query += " FROM foos" if b else " AS k FROM foos"
    query += "" if c else " ORDER BY k"
    # error:
    return fetch_all(query, tuple[int])
```

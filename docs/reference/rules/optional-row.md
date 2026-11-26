---
order: 355
group: "rewrite"
severity: "error"
fixable: true
summary: "SQL[X | None]: a row is never None"
origin: "Python"
---

# optional-row

Default severity: **error**

## What it does

Flags a row type that admits `None`: `SQL[X | None]`, `SQL[None | X]`,
`SQL[Optional[X]]` or `SQL[Union[X, None]]`. sqlty checks `X` against the query as
if the `| None` were not there.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from typing import NamedTuple, Optional

from sqlty import SQL


class User(NamedTuple):
    id: int
    name: str


# error: [optional-row] "SQL[User | None] should be SQL[User]: a row is never None"
by_id = SQL[User | None]("--sql\nSELECT id, name FROM users WHERE id = %s")
# error: [optional-row] "SQL[Optional[tuple[int]]] should be SQL[tuple[int]]: a row is never None"
ids = SQL[Optional[tuple[int]]]("--sql\nSELECT id FROM users")
```

## Why is this bad?

`R` is the type of each row, and a driver never returns `None` as a row: a query
that matches nothing returns no rows. `fetchone()` returns `R | None` on its
own, so `| None` on `R` adds nothing there, and for `fetchall()` it is a false
type the code then has to check for.

## Fix

`sqlty check --fix .` removes the `| None`:

```python
from typing import NamedTuple

from sqlty import SQL


class User(NamedTuple):
    id: int
    name: str


users = SQL[User | None]("--sql\nSELECT id, name FROM users")
# fixed: from typing import NamedTuple
# fixed:
# fixed: from sqlty import SQL
# fixed:
# fixed:
# fixed: class User(NamedTuple):
# fixed:     id: int
# fixed:     name: str
# fixed:
# fixed:
# fixed: users = SQL[User]("--sql\nSELECT id, name FROM users")
```

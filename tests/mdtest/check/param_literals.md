# Parameters compared with Literal columns

A column with a value set (a `CHECK ... IN`, an enum, a constant) gives its parameters the
type `str` or `int`, not a `Literal`: the check cannot know which value a caller passes,
and a `str` is always accepted.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid')),
    priority INTEGER NOT NULL CHECK (priority IN (1, 2, 3))
);
```

## Inferred type

```sql
SELECT id FROM orders WHERE status = %s AND priority = %s AND status = ANY(%s)
-- params: 1: str (TEXT)
-- params: 2: int (INT)
-- params: 3: list[str] (ARRAY<TEXT>)
```

## An arbitrary str argument is accepted

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

def by_status(status: str, priority: int) -> list[int]:
    return fetch_all("SELECT id FROM orders WHERE status = %s AND priority = %s", (status, priority), int)

known = fetch_all("SELECT id FROM orders WHERE status = %s", ("paid",), int)
unlisted = fetch_all("SELECT id FROM orders WHERE status = %s", ("unknown",), int)
# error: [param-type-mismatch] "parameter 1 expects str, got bytes"
wrong = fetch_all("SELECT id FROM orders WHERE status = %s", (b"paid",), int)
```

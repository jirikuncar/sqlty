# Declared parameter types

`SQL[R, P]` declares the types a call passes, and an argument must fit them as a type checker
reads it: a `bool` is an `int` or a `float`, an `int` a `float`, a `datetime` a `date`, a bare
`list` any list, and a type sqlty does not know anything.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score DOUBLE PRECISION, born DATE, seen TIMESTAMP, tags INTEGER[]);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## A declared type the argument must fit

```python
import datetime
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

def go(a: bool):
    if a:
        q = SQL[tuple[int], tuple[float]]("--sql\nSELECT id FROM users WHERE score = %s")
    else:
        q = SQL[tuple[int], tuple[float]]("--sql\nSELECT id FROM users WHERE score = %s")
    # error: [param-type-mismatch] "parameter 1 expects float, got str"
    fetch_all(q, ("x",), int)
    fetch_all(q, (1,), int)
    fetch_all(q, (True,), int)
    # error: [param-type-mismatch] "parameter 1 expects list, got int"
    fetch_all(SQL[tuple[int], tuple[list]]("--sql\nSELECT id FROM users WHERE tags = %s"), (1,), int)
    fetch_all(SQL[tuple[int], tuple[list]]("--sql\nSELECT id FROM users WHERE tags = %s"), ([1],), int)
    fetch_all(SQL[tuple[int], tuple[int]]("--sql\nSELECT id FROM users WHERE id = %s"), (True,), int)
    fetch_all(SQL[tuple[int], tuple[datetime.date]]("--sql\nSELECT id FROM users WHERE born = %s"), (datetime.datetime(2020, 1, 1),), int)
    # error: [param-type-mismatch] "parameter 1 expects datetime.datetime, got datetime.date"
    fetch_all(SQL[tuple[int], tuple[datetime.datetime]]("--sql\nSELECT id FROM users WHERE seen = %s"), (datetime.date(2020, 1, 1),), int)
    fetch_all(SQL[tuple[int], tuple[list[int]]]("--sql\nSELECT id FROM users WHERE tags = %s"), ([1],), int)
    # error: [param-type-mismatch] "parameter 1 expects list[int], got str"
    fetch_all(SQL[tuple[int], tuple[list[int]]]("--sql\nSELECT id FROM users WHERE tags = %s"), ("x",), int)
    # error: [param-type-mismatch] "parameter 1 expects list, got str"
    fetch_all(SQL[tuple[int], tuple[list]]("--sql\nSELECT id FROM users WHERE tags = %s"), ("x",), int)
    fetch_all(SQL[tuple[int], tuple[Mystery]]("--sql\nSELECT id FROM users WHERE name = %s"), (1,), int)
```

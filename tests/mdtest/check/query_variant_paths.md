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

## Guards that return add no paths

Each guard tests its condition once, so the paths that pass it need not remember how.

```python
from typing import LiteralString

from db import fetch_all


def find(
    by_n: bool,
    a0: bool, b0: bool,
    a1: bool, b1: bool,
    a2: bool, b2: bool,
    a3: bool, b3: bool,
    a4: bool, b4: bool,
    a5: bool, b5: bool,
    a6: bool, b6: bool,
    a7: bool, b7: bool,
    a8: bool, b8: bool,
    a9: bool, b9: bool,
    a10: bool, b10: bool,
    a11: bool, b11: bool,
    a12: bool, b12: bool,
) -> list[tuple[str]]:
    column: LiteralString = "id"
    if a0:
        if b0:
            return []
    if a1:
        if b1:
            return []
    if a2:
        if b2:
            return []
    if a3:
        if b3:
            return []
    if a4:
        if b4:
            return []
    if a5:
        if b5:
            return []
    if a6:
        if b6:
            return []
    if a7:
        if b7:
            return []
    if a8:
        if b8:
            return []
    if a9:
        if b9:
            return []
    if a10:
        if b10:
            return []
    if a11:
        if b11:
            return []
    if a12:
        if b12:
            return []
    if by_n:
        column = "n"
    # error: [column-type-mismatch] "tuple[str] item 0 is str but query column 'id' infers int"
    # error: [column-type-mismatch] "tuple[str] item 0 is str but query column 'n' infers int (variant: line 63)"
    return fetch_all("SELECT " + column + " FROM foos", tuple[str])
```

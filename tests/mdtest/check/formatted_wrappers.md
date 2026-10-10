# Formatter-Split Wrappers

Formatters split long wrappers over three lines. `check` reads them like
`rewrite` does: as wrapped, and checked as a whole. Diagnostics point at the
line with the wrapper.

## Current tuple type

```python
from sqlty import SQL

q = SQL[tuple[int, str]](
    "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
)
# error:
```

## Stale tuple type

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
q = SQL[tuple[int]](
    "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
)
```

## User type mismatch

```python
from typing import TypedDict

from sqlty import SQL


class User(TypedDict):
    id: str


# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User](
    "--sql\nSELECT id::INTEGER FROM users",
)
```

# column-count-mismatch

Default severity: **error**

## What it does

Checks that a user-defined row type (`TypedDict`, `NamedTuple`, dataclass,
Pydantic model) has as many fields as the query returns columns. A TypedDict, dataclass or
Pydantic model may omit fields with a default (`NotRequired` keys).

## Why is this bad?

Drivers build each row from the columns in order. A missing or extra field
fails at runtime, or silently drops data.

## Example

```python
from typing import TypedDict

from sqlty import SQL


class User(TypedDict):
    id: int


# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

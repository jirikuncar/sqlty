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

A TypedDict with `extra_items` ([PEP 728](https://peps.python.org/pep-0728/))
accepts the extra columns, as long as they have its type. `extra_items=object`
accepts any:

```python
from typing import TypedDict

from sqlty import SQL


class Item(TypedDict, extra_items=object):
    id: int
    name: str


# error:
q = SQL[Item]("--sql\nSELECT id::INTEGER, name::TEXT, owner_id::INTEGER FROM items")
```

See [Closed TypedDicts](../../../docs/guide/row-types.md#closed-typeddicts).

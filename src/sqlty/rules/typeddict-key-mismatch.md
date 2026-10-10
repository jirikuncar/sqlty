# typeddict-key-mismatch

Default severity: **error**

## What it does

For closed TypedDicts ([PEP 728](https://peps.python.org/pep-0728/):
`closed=True`, `extra_items=...`, or a subclass of one), matches query
columns to keys by name: every required key must be selected, and columns
without a key are allowed only through `extra_items`.

## Why is this bad?

A closed TypedDict promises exactly its keys; a row with other keys
violates that promise.

## Example

```python
from typing import NotRequired, TypedDict

from sqlty import SQL


class User(TypedDict, closed=True):
    id: int
    name: str
    bio: NotRequired[str]


# error: [typeddict-key-mismatch] "User.name is required but missing from query"
missing = SQL[User]("--sql\nSELECT id::INTEGER, bio::TEXT FROM users")
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
extra = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
```

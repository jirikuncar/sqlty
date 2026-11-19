# typeddict-key-mismatch

Default severity: **error**

## What it does

Matches query columns to the fields of a TypedDict, dataclass or Pydantic
model by name, as `dict_row` and `class_row` do: every required field must be
selected, and columns without a field are allowed only through
[PEP 728](https://peps.python.org/pep-0728/) `extra_items=...` or a Pydantic
model's `extra="allow"`. `NotRequired` keys and fields with a default may be
missing.

## Why is this bad?

A row factory that passes columns by name fails on a missing required field
and on a column the type does not declare. A Pydantic model drops an undeclared column by
default, so the query fetches data, possibly sensitive, that is never used.

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

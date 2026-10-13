# column-type-mismatch

Default severity: **error**

## What it does

Checks each field type of a user-defined row type against the type inferred
for the matching query column.

## Why is this bad?

The annotation promises a type the database won't return, so type checkers
validate code against the wrong row.

## Example

```python
from datetime import datetime
from typing import NamedTuple

from sqlty import SQL


class Event(NamedTuple):
    id: int
    created_at: str


# error: [column-type-mismatch] "Event.created_at is str but query column 'created_at' infers datetime.datetime"
q = SQL[Event]("--sql\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
```

`from datetime import datetime` and `datetime.datetime` are equivalent: types
are resolved, not compared by spelling.

A field passes when it can hold every value of its column: `Any`, a union
containing `Any` or the column type, and `float` for an `int` column are
accepted. A narrower field, such as `bool` for an `INTEGER` column, is not.

```python
from typing import Any, NamedTuple

from sqlty import SQL


class Row(NamedTuple):
    id: Any
    total: float | None
    active: bool


# error: [column-type-mismatch] "Row.active is bool but query column 'active' infers int"
q = SQL[Row]("--sql\nSELECT id::INTEGER, total::INTEGER, active::INTEGER FROM orders")
```

A `NewType` field is accepted wherever its supertype is, and a parametrised
generic such as `list[str]` for a column that infers `list` (array element
types are inferred but not compared):

```python
from typing import NamedTuple, NewType

from sqlty import SQL

UserId = NewType("UserId", int)


class User(NamedTuple):
    id: UserId
    tags: list[str]


q = SQL[User]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# error:
```

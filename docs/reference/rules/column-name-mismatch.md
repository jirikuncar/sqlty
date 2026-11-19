# column-name-mismatch

Default severity: **warn**

## What it does

Checks that the fields of a `NamedTuple` are named like the query columns,
in order. `NamedTuple` is matched by position; TypedDicts, dataclasses and
Pydantic models are matched by name (see
[`typeddict-key-mismatch`](typeddict-key-mismatch.md)).

## Why is this bad?

Name-based row factories (`dict_row`, `class_row`) need matching names, and a
positional mismatch usually means two columns were swapped.

## Example

```python
from typing import NamedTuple

from sqlty import SQL


class User(NamedTuple):
    name: str
    email: str


# error: [column-name-mismatch] "User.email doesn't match query column 'login'"
q = SQL[User]("--sql\nSELECT name::TEXT, login::TEXT FROM users")
```

Alias the column (`login AS email`) or rename the field.

# column-name-mismatch

Default severity: **warn**

## What it does

Checks that the fields of a user-defined row type are named like the query
columns, in order.

## Why is this bad?

Name-based row factories (`dict_row`, `class_row`) need matching names, and a
positional mismatch usually means two columns were swapped.

## Example

```python
from dataclasses import dataclass

from sqlty import SQL


@dataclass
class User:
    name: str
    email: str


# error: [column-name-mismatch] "User.email doesn't match query column 'login'"
q = SQL[User]("--sql\nSELECT name::TEXT, login::TEXT FROM users")
```

Alias the column (`login AS email`) or rename the field.

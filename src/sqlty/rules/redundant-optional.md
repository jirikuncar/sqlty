# redundant-optional

Default severity: **ignore**

## What it does

Reports a field of a user-defined row type (or an item of a helper's `tuple[...]` row
type) annotated `T | None` or `Optional[T]` while its query column cannot be NULL. This is
the strict side of `nullable-mismatch`: that rule finds missing `| None`, this one finds
extra.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-optional = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from typing import TypedDict

from sqlty import SQL


class User(TypedDict):
    id: int
    name: str | None


# error: [redundant-optional] "User.name is str | None but query column 'name' is never NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Why is this bad?

Callers handle a `None` that never arrives, and the annotation hides which columns are
really optional.

## How to fix

Drop the `| None`, or accept it: a wider annotation is correct, just looser than needed.

```python
from typing import TypedDict

from sqlty import SQL


class User(TypedDict):
    id: int
    name: str


q = SQL[User]("--sql\nSELECT id, name FROM users")
# error:
```

## Configuration

Off by default, also in preview, because `T | None` on a non-null column is correct, only
loose. Enable it:

```toml
[tool.sqlty.rules]
redundant-optional = "warn"
```

Only columns sqlty knows cannot be NULL are judged: a schema column with `NOT NULL` /
`PRIMARY KEY`, a `COUNT`, a literal. A column of unknown nullability (a cast with no
schema, an unknown function) is never reported. `Any` and `object` fields are not
optional annotations and are not reported.

---
order: 440
group: "params"
severity: "error"
fixable: false
summary: "P of SQL[R, P] doesn't fit the query's parameters"
origin: "Python"
---

# params-type-mismatch

Default severity: **error**

## What it does

Checks the parameter type `P` of a `SQL[R, P]` query against the parameters of
the query, as [`param-type-mismatch`](param-type-mismatch.md) checks the values a
call passes:

- A `tuple` needs one item per positional placeholder, and `tuple[()]` none.
- `NoParams` fits only a query without placeholders.
- A `TypedDict` needs a required key per named placeholder, and no other key.
- An item or key whose type can't be the type of its parameter is reported. So
  is one that allows `None` for a parameter stored in a `NOT NULL` column.
- A `Literal[...]` item or key with a value the column of its parameter never
  holds (an enum or a `CHECK` list) is reported: the database rejects the
  write, or the comparison matches no row. The parameter must be stored in the
  column, or compared with it by `=` or `IN` only. PostgreSQL and SQLite only,
  as MySQL compares without case.

A type alias (`X = T`, `X: TypeAlias = T` or `type X = T`, in the module or
imported) is checked as the type `T` it names.

A `str` and a cast parameter take anything, and `typing.Any`, an unknown type or
an annotation `P` can't be read from are never reported. With
`dialect = "sqlite"` only `None` and `Literal` values are checked. A `P` that
[`param-types`](../../reference/configuration.md#param-types) writes
always fits.

## Why is this bad?

Your type checker checks the values you pass against `P`, not against the
query. A `P` that doesn't fit lets wrong values through, or rejects right ones.

## Example

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    born DATE,
    role TEXT CHECK (role IN ('admin', 'member'))
);
```

```python
import datetime
from typing import Literal, TypedDict

from sqlty import SQL


class ById(TypedDict):
    user_id: int


# error: [params-type-mismatch] "ById has no key for query parameter 'id'"
# error: [params-type-mismatch] "ById.user_id is not a query parameter"
by_id = SQL[tuple[str], ById]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# error: [params-type-mismatch] "tuple[datetime.date, int] has 2 items but the query has 1 parameter"
born = SQL[tuple[str], tuple[datetime.date, int]]("--sql\nSELECT name FROM users WHERE born > %s")
# error: [params-type-mismatch] "tuple[int, str | None] item 2 is str | None but query parameter 2 is not nullable"
named = SQL[tuple[int], tuple[int, str | None]]("--sql\nINSERT INTO users (id, name) VALUES (%s, %s) RETURNING id")
# error: [params-type-mismatch] "tuple[Literal['owner']] item 1 is Literal['owner'] but query parameter 1 is never 'owner'"
owners = SQL[tuple[int], tuple[Literal["owner"]]]("--sql\nSELECT id FROM users WHERE role = %s")
```

## Fix

Run `sqlty strip` and `sqlty rewrite` on the query to write `P` again, or
change `P` to match the query. Turn the rule off with
`params-type-mismatch = "ignore"` in `[tool.sqlty.rules]`.

# params-type-mismatch

Default severity: **error**

## What it does

Checks the parameter type `P` of a `SQL[R, P]` query against the parameters of
the query, as [`param-type-mismatch`](param-type-mismatch.md) checks the values a
call passes:

- A `tuple` needs one item per positional placeholder, and `tuple[()]` none.
- A `TypedDict` needs a required key per named placeholder, and no other key.
- An item or key whose type can't be the type of its parameter is reported. So
  is one that allows `None` for a parameter stored in a `NOT NULL` column.

A `str` and a cast parameter take anything, and `typing.Any`, an unknown type or
an annotation `P` can't be read from are never reported. With
`dialect = "sqlite"` only `None` is checked. A `P` that
[`param-types`](../../../docs/reference/configuration.md#param-types) writes
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
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, born DATE);
```

```python
import datetime
from typing import TypedDict

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
```

## Fix

Run `sqlty rewrite` with `param-types = true` on the query without `P`, or
change `P` to match the query. Turn the rule off with
`params-type-mismatch = "ignore"` in `[tool.sqlty.rules]`.

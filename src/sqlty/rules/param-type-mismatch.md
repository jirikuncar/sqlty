# param-type-mismatch

Default severity: **error**

## What it does

Checks the arguments a call passes for its query's placeholders against the
type each placeholder takes from its context: the column it is compared with
or stored in, a cast, `LIMIT`, an `IN` list, `= ANY(%s)` (a list). It applies
to calls of a [marked helper](../../../docs/guide/helpers.md) whose parameters are spelled
out: a `params` argument holding a tuple, list or dict literal, `*args`, or
`**params`.

An argument is judged only where the source states its type: a literal, a
variable annotated or assigned a class, a call of a known class (`date(...)`,
`UUID(...)`). An argument of unknown type is never reported, and neither is a
`str`, which the server coerces to any type. `None` is reported only for a
parameter stored in a `NOT NULL` column.

## Why is this bad?

The database rejects the value, or compares it in a way that never matches,
at run time and only on the path that runs the query.

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
from datetime import date
from typing import Annotated, LiteralString

from sqlty import SQL


def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...


execute("SELECT name FROM users WHERE id = %s AND born > %s", (1, date(2000, 1, 1)))
# error: [param-type-mismatch] "parameter 1 expects int, got datetime.date"
execute("SELECT name FROM users WHERE id = %s", (date(2000, 1, 1),))
# error: [param-type-mismatch] "parameter 2 is not nullable, got None"
execute("INSERT INTO users (id, name) VALUES (%s, %s)", (1, None))
# error: [param-type-mismatch] "parameter 'id' expects int, got float"
execute("DELETE FROM users WHERE id = %(id)s", {"id": 1.5})
```

## Fix

Pass a value of the type the query expects, or change the query.
Turn the rule off with `param-type-mismatch = "ignore"` in
`[tool.sqlty.rules]`.

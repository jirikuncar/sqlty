# untyped-query

Default severity: **warn**

## What it does

Flags `SQL[UNTYPED]` queries, written by `sqlty rewrite --untyped sentinel`
for queries without an inferable row type, and says when one has become
inferable.

## Why is this bad?

`UNTYPED` rows can't be used as a concrete type; the query still needs casts
or a schema.

## Example

```python
from sqlty import SQL, UNTYPED

# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
values = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[int]]; run sqlty check --fix"
cast = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
```

## Fix

`sqlty check --fix .` replaces `UNTYPED` once the row type is inferable:

```python
from sqlty import SQL
from sqlty import UNTYPED

q = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

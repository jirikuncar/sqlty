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
values = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[int]]; strip and rewrite to update"
cast = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
```

# stale-type

Default severity: **warn**

## What it does

Flags `SQL[tuple[...]]` annotations that no longer match the query.

## Why is this bad?

The SQL changed after the last rewrite, so the annotation describes rows the
query no longer returns.

## Example

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Run `sqlty strip` and `sqlty rewrite` on the file. User-defined row types are
checked by the `column-*` rules instead.

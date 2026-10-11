# stale-type

Default severity: **warn**

## What it does

Flags `SQL[tuple[...]]` annotations that no longer match the query. An item
that only parametrises an inferred bare generic (`list[str]` for `list`) is
more precise than inference and is not flagged.

## Why is this bad?

The SQL changed after the last rewrite, so the annotation describes rows the
query no longer returns.

## Example

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

User-defined row types are checked by the `column-*` rules instead.

## Fix

`sqlty check --fix .` writes the inferred type:

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Parametrised generics that refine the inferred type are kept by `--fix`:

```python
from sqlty import SQL

q = SQL[tuple[int, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
```

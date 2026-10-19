# narrow-type

Default severity: **error**

## What it does

Flags `SQL[tuple[...]]` annotations that don't admit every row the query
returns: the SQL type is wider than the annotation. That is a column made
nullable under `str`, a column added to the query, or a column whose type
changed.

An annotation that admits every row but is wider than it needs to be is
[`stale-type`](stale-type.md) instead.

## Why is this bad?

The code gets rows its annotation says it can't: a `None` it doesn't check for,
a tuple it unpacks into too few names, an `int` it treats as a `str`. The type
checker trusts the annotation, so it doesn't catch the failure.

## Example

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

User-defined row types are checked by the `column-*` rules and
`nullable-mismatch` instead.

## Fix

`sqlty check --fix .` writes the inferred type, and your type checker then finds
the code that needs to handle it:

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

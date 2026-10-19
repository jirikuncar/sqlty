# stale-type

Default severity: **error**

## What it does

Flags `SQL[tuple[...]]` annotations that no longer match the query. An item
that only parametrises an inferred generic (`list[str]` for `list`), or narrows
an inferred `typing.Any` (a JSON column), is more precise than inference and is
not flagged.

## Why is this bad?

The SQL changed after the last rewrite, so the annotation describes rows the
query no longer returns. When the code can now get a value its annotation
doesn't admit, such as `None` from a column made nullable, it is an error.

An annotation that admits every row the query returns, only wider than it needs
to be (`str | None` for a column made `NOT NULL`, or `object`), still works, so it
is a warning. Setting `stale-type` in `[tool.sqlty.rules]` gives every
`stale-type` that severity.

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

# stale-type

Default severity: **warn**

## What it does

Flags `SQL[tuple[...]]` annotations that admit every row the query returns but
no longer match it: `str | None` for a column made `NOT NULL`, `object`, or `str`
for a column of `Literal` values. An item that only parametrises an inferred
generic (`list[str]` for `list`), or narrows an inferred `typing.Any` (a JSON
column), is more precise than inference and is not flagged.

An annotation that doesn't admit every row is
[`narrow-type`](narrow-type.md) instead.

## Why is this bad?

The SQL or the schema changed after the last rewrite, so the annotation is wider
than it needs to be. The code still works, but handles values, such as `None`,
that never come.

## Example

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int | None]] should be SQL[tuple[int]]"
q = SQL[tuple[int | None]]("--sql\nSELECT id::INTEGER FROM users")
```

User-defined row types are checked by the `column-*` rules and
`redundant-optional` instead.

## Fix

`sqlty check --fix .` writes the inferred type:

```python
from sqlty import SQL

q = SQL[tuple[int | None]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

Parametrised generics that refine the inferred type are kept by `--fix`:

```python
from sqlty import SQL

q = SQL[tuple[int, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
```

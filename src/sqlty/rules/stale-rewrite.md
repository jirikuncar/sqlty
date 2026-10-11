# stale-rewrite

Default severity: **warn**

## What it does

Flags `--sql` strings that `sqlty rewrite` would wrap.

## Why is this bad?

An unwrapped query is a plain `str`: type checkers and drivers see no row
type.

## Example

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT id::INTEGER FROM users"
```

## Fix

`sqlty rewrite .` or `sqlty check --fix .`:

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

Strings rewrite can't type (DDL, DML without `RETURNING`, invalid SQL) are
not flagged; see [untyped queries](../../../docs/guide/untyped.md) to wrap
them anyway.

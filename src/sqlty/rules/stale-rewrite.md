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

## Untyped `SQL(...)` calls

A query passed to sqlty's `SQL` without a row type is flagged too, under any
name `SQL` is imported as. Without the `--sql` marker, `check --fix` adds it:

```python
from sqlty import SQL

# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q1 = SQL("--sql\nSELECT id::INTEGER FROM users")
# error: [stale-rewrite] "SQL(...) has no row type; it needs a --sql marker and SQL[T]"
q2 = SQL("SELECT id::INTEGER, name::TEXT FROM users")
```

```python
from sqlty import SQL

q1 = SQL("--sql\nSELECT id::INTEGER FROM users")
q2 = SQL("SELECT id::INTEGER, name::TEXT FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q1 = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: q2 = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

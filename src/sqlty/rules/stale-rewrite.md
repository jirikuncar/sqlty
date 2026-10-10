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

Run `sqlty rewrite .`.

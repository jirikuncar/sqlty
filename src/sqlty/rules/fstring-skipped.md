# fstring-skipped

Default severity: **warn**

## What it does

Flags f-strings starting with `--sql`, which rewrite leaves alone.

## Why is this bad?

Interpolated SQL can't be analyzed statically, and string formatting is the
classic route to SQL injection.

## Example

```python
# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
q = f"--sql\nSELECT {column}::INTEGER FROM users"
```

Pass values as query parameters; build identifiers with your driver's SQL
composition (`psycopg.sql`).

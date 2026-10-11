# inline-sql-comment

Default severity: **warn**

## What it does

Flags `"--sql SELECT ..."`, where a space instead of a newline follows the
marker.

## Why is this bad?

`--sql` starts a SQL comment that runs to the end of the line, so the whole
query is a comment: nothing is inferred and the database receives an empty
statement.

## Example

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT id::INTEGER FROM users"
```

Use `"--sql\nSELECT ..."`.

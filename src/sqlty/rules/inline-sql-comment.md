# inline-sql-comment

Default severity: **warn**

## What it does

Flags `"--sql SELECT ..."`, where a space instead of a newline follows the
marker and a SQL statement follows the space.

## Why is this bad?

`--sql` starts a SQL comment that runs to the end of the line, so the whole
query is a comment: nothing is inferred and the database receives an empty
statement.

## Example

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT id::INTEGER FROM users"
```

Use `"--sql\nSELECT ..."`. Only a statement after the space is reported
(`SELECT` with a column, `DELETE FROM`, `UPDATE t SET`, `CREATE TABLE`, ...),
so text such as `"--sql flag enables it"` is not. `sqlty check --fix` makes
the change when the string is a plain literal (not raw, f- or t-) that this
rule reports.

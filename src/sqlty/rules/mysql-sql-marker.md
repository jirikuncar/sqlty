# mysql-sql-marker

Default severity: **warn**

## What it does

With `dialect = "mysql"`, flags strings that open with the `--sql` marker.

## Why is this bad?

MySQL and MariaDB read `--` as a comment only when whitespace follows it.
`--sql` is read as two minus signs and a name, so the server rejects the query
with a syntax error (1064), although sqlty types it.

## Example

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [mysql-sql-marker] "MySQL reads "--sql" as an expression, not a comment; use "-- sql""
q = SQL[tuple[int]]("--sql\nSELECT CAST(id AS SIGNED) FROM users")
```

Use `"-- sql\nSELECT ..."`, a comment in every dialect, which sqlty also
reads as the marker. `sqlty check --fix` makes the change in plain string
literals, wrapped or not.

# select-star

Default severity: **warn**

The examples use this schema:

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

## What it does

Flags `SELECT *` in queries whose columns come from a schema.

## Why is this bad?

The row type is fixed when the query is rewritten. Adding a column to the
table silently changes what the query returns at runtime.

## Example

```python
from sqlty import SQL

# error: [select-star] "SELECT * used — expand to explicit column list (joins need a manual edit)"
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
```

## Fix

`sqlty check --fix .` lists the columns from the schema:

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INT, name::TEXT FROM users")
```

Queries with escapes or quotes inside the string, and queries that read from
more than one table (`JOIN` or `FROM a, b`; the expansion drops table
qualifiers), are left for you to edit. `select-star` is still reported for them.

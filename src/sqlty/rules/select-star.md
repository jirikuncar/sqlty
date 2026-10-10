# select-star

Default severity: **warn**

## What it does

Flags `SELECT *` in queries whose columns come from a schema.

## Why is this bad?

The row type is fixed when the query is rewritten. Adding a column to the
table silently changes what the query returns at runtime.

## Example

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [select-star] "SELECT * used — expand to explicit column list"
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
```

List the columns instead; `sqlty.core.expand_star()` produces the list.

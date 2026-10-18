# unknown-function

Default severity: **warn**

## What it does

Reports a function call whose result sqlty cannot type because it does not know the function. The column is left untyped.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-function = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [unknown-function] "function `my_udf` is not known to sqlty"
q = SQL[tuple[int]]("--sql\nSELECT my_udf(id) FROM users")
```

## Why is this bad?

A function sqlty does not know gives the column no type, so the annotation is not checked.

## How to fix

Cast the call, `my_udf(1)::INTEGER`, which also types it. A built-in function missing from sqlty's table is a gap in sqlty: please report it. To accept one case, put `# sqlty: ignore[unknown-function]` on its line.

## Configuration

A warning by default: the SQL is valid, but sqlty cannot type the function's result, so the query is
not fully checked. A warning tells you why without failing CI on sqlty's gaps. Make it
fail, or turn it off:

```toml
[tool.sqlty.rules]
unknown-function = "error"
```

Only checked when a schema with tables is configured: without one every user function would be unknown. A function the schema declares with `CREATE FUNCTION` is known.

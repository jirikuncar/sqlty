# unknown-function

Default severity: **ignore**

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

Off by default, because it reports code that passes today. Enable it:

```toml
[tool.sqlty.rules]
unknown-function = "warn"
```

Only checked when a schema with tables is configured: without one every user function would be unknown. User-defined functions in the schema file are not read yet, so they are reported too. `error` makes `sqlty check` fail on it.

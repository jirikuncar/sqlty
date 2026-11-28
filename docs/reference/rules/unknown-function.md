---
order: 220
group: "names"
severity: "warn"
fixable: true
summary: "Function whose result sqlty cannot type"
origin: "SQL"
---

# unknown-function

Default severity: **warn**

## What it does

Reports a function call whose result sqlty cannot type because it does not know the function. The column is left untyped. A call in `WHERE`, `JOIN ... ON`, `ORDER BY` or a subquery is reported too: it has no column to type, but the function still has to exist when the query runs. A function in `FROM` that is neither one of the dialect's built-ins nor one the schema defines is reported with the table function it most likely misspells, which `--fix` writes.

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

```python
from sqlty import SQL

# error: [unknown-function] "table function `generate_seriez` is not known to sqlty; did you mean `generate_series`?"
q = SQL[tuple[int]]("--sql\nSELECT n FROM generate_seriez(1, 3) AS n")
```

## Why is this bad?

A function sqlty does not know gives the column no type, so the annotation is not checked. Called anywhere in the query, a function the schema no longer defines fails at run time, and `sqlty breaking` reports the calls a migration that drops it would break.

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

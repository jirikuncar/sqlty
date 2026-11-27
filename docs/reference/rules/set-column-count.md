---
order: 261
group: "names"
severity: "error"
fixable: false
summary: "UNION, INTERSECT or EXCEPT branch with a different number of columns than the first"
origin: "SQL"
---

# set-column-count

Default severity: **error**

## What it does

Reports a branch of a `UNION`, `INTERSECT` or `EXCEPT` that returns a different
number of columns than the first branch, on that branch.

```python
from sqlty import SQL

# error: [set-column-count] "each UNION branch must return 2 columns, this one returns 1"
q = SQL[tuple[int, str]]("--sql\nSELECT 1, 'a' UNION SELECT 2")
```

## Why is this bad?

Every database rejects the query: PostgreSQL with `each UNION query must have
the same number of columns`, MySQL with `The used SELECT statements have a
different number of columns`, SQLite with `SELECTs to the left and right of
UNION do not have the same number of result columns`.

## How to fix

Select the same columns in each branch, padding one with `NULL` where it has
no value for a column.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
set-column-count = "warn"
```

Needs no schema; a `SELECT *` is counted once a schema expands it. A branch
whose width sqlty can't tell, such as `SELECT *` over a table it doesn't know,
is not checked.

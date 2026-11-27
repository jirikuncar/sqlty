---
order: 263
group: "names"
severity: "warn"
fixable: false
summary: "CTE that no later CTE or the statement reads"
origin: "SQL"
---

# unused-cte

Default severity: **warn**

## What it does

Reports a CTE that neither a later CTE nor the statement reads, on its name. Editors
fade it as unnecessary code.

```python
from sqlty import SQL

# error: [unused-cte] "CTE `unused` is never read"
q = SQL[tuple[int]]("--sql\nWITH unused AS (SELECT 1) SELECT 2")
```

A data-modifying CTE runs whether read or not, so it is not reported:

```python
from sqlty import SQL

# error:
q = SQL[tuple[int]]("--sql\nWITH gone AS (DELETE FROM users RETURNING id) SELECT 1")
```

## Why is this bad?

The database never runs an unread `SELECT` CTE
([PostgreSQL](https://www.postgresql.org/docs/17/queries-with.html#QUERIES-WITH-MODIFYING)):
it is dead code, often left over from an edit, that readers still have to follow.

## How to fix

Remove the CTE, or read it where it was meant to be read.

## Configuration

A warning by default: the query runs. Turn it into an error or off:

```toml
[tool.sqlty.rules]
unused-cte = "error"
```

Needs no schema. A name resolves to the innermost CTE of that name, and a recursive
CTE reading itself counts as read.

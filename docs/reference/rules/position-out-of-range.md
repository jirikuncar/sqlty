---
order: 262
group: "names"
severity: "error"
fixable: false
summary: "ORDER BY or GROUP BY position that names no column of the select list"
origin: "SQL"
---

# position-out-of-range

Default severity: **error**

## What it does

Reports an integer in `ORDER BY` or `GROUP BY` that names no column of the
select list: 0, a negative number, or one larger than the number of columns.
`ORDER BY` of a set operation counts the columns of its first branch.

```python
from sqlty import SQL

# error: [position-out-of-range] "ORDER BY position 5 is not in the select list (2 columns)"
q = SQL[tuple[int, str]]("--sql\nSELECT 1, 'a' ORDER BY 5")
```

## Why is this bad?

The database rejects the query. PostgreSQL reports `ORDER BY position 5 is not
in select list`, MySQL `Unknown column '5' in 'order clause'`, SQLite
`1st ORDER BY term out of range - should be between 1 and 2` and DuckDB `ORDER
term out of range - should be between 1 and 2`.

## How to fix

Name the column, or count from 1 to the number of columns the query selects.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
position-out-of-range = "warn"
```

Needs no schema; a `SELECT *` is counted once a schema expands it. A negative
position is not judged in the `mysql` dialect, which reads it as an expression. `ungrouped-column` is not
reported for a query whose `GROUP BY` position is out of range, unless its select list has a `*`.
An integer wider than 32 bits is a constant outside DuckDB, and `+5` a position only in SQLite.

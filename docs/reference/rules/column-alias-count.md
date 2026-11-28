---
order: 264
group: "names"
severity: "error"
fixable: false
summary: "Column alias list that does not fit the relation it renames"
origin: "SQL"
---

# column-alias-count

Default severity: **error**

## What it does

Reports a column alias list, such as `AS x(a, b, c)` or `WITH t(a, b)`, with more
names than the subquery, table, view, table function or CTE it renames has columns.

```python
from sqlty import SQL

# error: [column-alias-count] "`x` has 1 column but 3 aliases"
q = SQL[tuple[int]]("--sql\nSELECT x.a FROM (SELECT 1) AS x(a, b, c)")
```

## Why is this bad?

The database rejects the query. PostgreSQL reports `table "x" has 1 columns
available but 3 columns specified`, and DuckDB the same for a subquery or table.
MySQL and SQLite also reject a list shorter than the relation: MySQL wants as many
names as columns for a CTE or derived table, SQLite reports `table t has 2 values for
1 columns` for a CTE.

## How to fix

Name each column once, or select the columns the list names.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
column-alias-count = "warn"
```

A shorter list is reported only in the `mysql` and `sqlite` dialects; PostgreSQL and
DuckDB keep the names of the remaining columns. A DuckDB CTE or table function is not
judged, as DuckDB accepts a longer list there. A relation of unknown width is skipped,
and so is a function returning one composite value, whose fields the list names. SQLite
rejects a CTE only once the query reads it; sqlty reports any, and
[`unused-cte`](unused-cte.md) the one that is not read.

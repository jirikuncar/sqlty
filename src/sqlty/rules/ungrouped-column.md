# ungrouped-column

Default severity: **error**

## What it does

Reports a column that a grouped query selects without grouping by it or
aggregating it. A query is grouped by a `GROUP BY`, or by an aggregate such as
`count(*)` in its select list.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
ungrouped-column = "warn"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT NOT NULL, total INTEGER NOT NULL);
```

```python
from sqlty import SQL

# error: [ungrouped-column] "column `orders.total` must appear in GROUP BY or be used in an aggregate function"
q = SQL[tuple[str, int]]("--sql\nSELECT status, total FROM orders GROUP BY status")
```

A column is grouped when a key names it, by position (`GROUP BY 1`) or by its
result alias too, or when a key is the expression it sits in. Grouping by a
table's primary key covers the table's other columns, as PostgreSQL's functional
dependency does:

```python
from sqlty import SQL

# error:
by_id = SQL[tuple[int, str, int]]("--sql\nSELECT id, status, count(*) FROM orders GROUP BY id")
by_position = SQL[tuple[str, int]]("--sql\nSELECT status, count(*) FROM orders GROUP BY 1")
```

`*`, `t.*` and a whole-row reference such as `row_to_json(o)` read every column
of their table, so each column must be grouped, or the table's primary key.

## Why is this bad?

PostgreSQL rejects the query: `column "orders.total" must appear in the GROUP BY
clause or be used in an aggregate function`. Each group has one row in the
result, so a column that varies within a group has no single value.

## How to fix

Group by the column, or aggregate it: `sum(total)`, `max(total)`.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
ungrouped-column = "warn"
```

Needs a [schema](../../../docs/guide/schema-inference.md): only columns of
schema tables are judged. Judged in the `postgres` dialect; MySQL's
`ONLY_FULL_GROUP_BY` follows more dependencies (unique keys, equalities in
`WHERE`) and is not modelled.

---
order: 266
group: "names"
severity: "warn"
fixable: false
summary: "WHERE compares a column with a constant its enum or CHECK list never holds"
origin: "SQL"
---

# unmatched-literal

Default severity: **warn**

## What it does

Reports a `WHERE` of a `SELECT`, an `UPDATE` or a `DELETE` that compares a column whose
values are known, from an enum or a `CHECK (column IN (...))`, with constants it never
holds: `column = constant` or `column IN (constants)`, as a term of the `AND` of the
`WHERE`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id SERIAL PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid'))
);
```

```python
from sqlty import SQL

# error: [unmatched-literal] "`orders.status` is never 'refunded', so this WHERE matches no row"
refunded = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE status = 'refunded'")
```

## Why is this bad?

The query runs, and returns, updates or deletes nothing: usually a typo, or a value a
migration removed. An enum is worse: PostgreSQL rejects a label the type does not have.

## How to fix

Compare with one of the listed values, or add the value to the enum or the `CHECK`:

```python
from sqlty import SQL

# error:
paid = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE status = 'paid'")
```

## Configuration

A warning by default: the query is valid SQL. Raise it or turn it off:

```toml
[tool.sqlty.rules]
unmatched-literal = "error"
```

Needs a [schema](../../guide/schema-inference.md), and is judged in the `postgres` and
`sqlite` dialects, where comparison is exact. A comparison under `OR` or `NOT` is not
judged, as the rest of the `WHERE` may still match.

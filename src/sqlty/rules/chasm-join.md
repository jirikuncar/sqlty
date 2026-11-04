# chasm-join

Default severity: **warn**

## What it does

Reports an aggregating query that joins one table one-to-many to two others
that don't join each other: each row of one branch repeats once per row of
the other.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY);
CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
CREATE TABLE payments (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
```

```python
import decimal

from sqlty import SQL

# error: [chasm-join] "orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"
# error: [fanout-aggregate] "SUM(i.amount) aggregates order_items, whose rows repeat once per payments row"
# error: [fanout-aggregate] "SUM(p.amount) aggregates payments, whose rows repeat once per order_items row"
query = SQL[tuple[int, decimal.Decimal, decimal.Decimal]]("""--sql
SELECT o.id, SUM(i.amount), SUM(p.amount)
FROM orders o
JOIN order_items i ON i.order_id = o.id
JOIN payments p ON p.order_id = o.id
GROUP BY o.id""")
```

## Why is this bad?

An order with three items and two payments gives six joined rows: each item
twice, each payment three times. No grouping undoes it, and `COUNT(*)` counts
the six rows.

## How to fix

Aggregate each branch in its own CTE, one row per order, then join them.

```python
from decimal import Decimal

from sqlty import SQL

# error:
query = SQL[tuple[int, Decimal | None, Decimal | None]]("""--sql
WITH items AS (SELECT order_id, SUM(amount) AS amount FROM order_items GROUP BY order_id),
     paid AS (SELECT order_id, SUM(amount) AS amount FROM payments GROUP BY order_id)
SELECT o.id, i.amount, p.amount
FROM orders o
JOIN items i ON i.order_id = o.id
JOIN paid p ON p.order_id = o.id""")
```

## Configuration

A warning by default, reported once per query. Each aggregate it inflates is
also a [`fanout-aggregate`](fanout-aggregate.md).

```toml
[tool.sqlty.rules]
chasm-join = "error"
```

Needs a [schema](../../../docs/guide/schema-inference.md) with primary or
unique keys, read as `fanout-aggregate` reads them.

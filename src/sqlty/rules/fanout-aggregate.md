# fanout-aggregate

Default severity: **warn**

## What it does

Reports an aggregate over a table whose rows a one-to-many join repeats.
`SUM`, `AVG`, `COUNT`, `STRING_AGG`, `ARRAY_AGG` and their kin count each
repeated row again.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY, segment TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers, total NUMERIC NOT NULL);
CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
```

```python
import decimal

from sqlty import SQL

# error: [fanout-aggregate] "AVG(o.total) aggregates orders, whose rows repeat once per order_items row"
query = SQL[tuple[str, decimal.Decimal, decimal.Decimal]]("""--sql
SELECT c.segment, AVG(o.total), SUM(i.amount)
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN order_items i ON i.order_id = o.id
GROUP BY c.segment""")
```

`SUM(i.amount)` is not reported: each item joins one order and one customer,
so items are not repeated.

## Why is this bad?

Joining `orders` to `order_items` repeats each order once per item, so
`AVG(o.total)` weighs an order by its number of items. The query is valid SQL,
every column types, and the result is wrong.

## How to fix

Aggregate each table at its own grain before joining: sum the items per order
in a CTE, then join one row per order.

```python
from decimal import Decimal

from sqlty import SQL

# error:
query = SQL[tuple[str, Decimal | None, Decimal | None]]("""--sql
WITH items AS (
    SELECT order_id, SUM(amount) AS amount FROM order_items GROUP BY order_id
)
SELECT c.segment, AVG(o.total), SUM(i.amount)
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN items i ON i.order_id = o.id
GROUP BY c.segment""")
```

## How sqlty decides

A join is to-one toward a table when its conditions equate, or fix to a
constant, every column of one of the table's keys: the primary key, a `UNIQUE`
constraint or a unique index without `WHERE`. The `GROUP BY` or `DISTINCT`
columns of a CTE or derived table are its key, and so is the key of the one
table it selects from, when it selects it.

A table's rows repeat when a table it reaches through to-one joins is joined to
another one whose key the join does not match, but whose own join matches the
first table's key: a parent joined to its children. A join that matches no key
on either side repeats nothing here; [`unproven-join`](unproven-join.md)
reports it.

`MIN`, `MAX`, `BOOL_AND`, `BOOL_OR` and `DISTINCT` aggregates such as
`COUNT(DISTINCT o.id)` give the same result over repeated rows and are not
reported. Nor is `COUNT(*)`, which counts the joined rows;
[`chasm-join`](chasm-join.md) reports a query where every table repeats.

## Configuration

A warning by default: the SQL runs, and sqlty only knows the keys the schema
declares. Make it fail CI:

```toml
[tool.sqlty.rules]
fanout-aggregate = "error"
```

Needs a [schema](../../../docs/guide/schema-inference.md) with primary or
unique keys. Judged in the `postgres`, `mysql` and `sqlite` dialects.

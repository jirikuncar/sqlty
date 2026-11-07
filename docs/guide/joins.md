---
description: "Catch totals a join double-counts and joins on the wrong key: the sqlty rules that read primary, unique and foreign keys from your schema to check joins without a database."
---

# Joins and aggregates

A query that joins orders to their items and averages the order totals is valid
SQL, every column types, and the result is wrong: each order counts once per
item. sqlty reads the primary, unique and foreign keys of your schema and
reports what a join does to aggregates and what it compares. It runs no query,
so the checks work offline, in an editor and in CI.

| Rule | Default | Reports |
|------|---------|---------|
| [`fanout-aggregate`](../reference/rules.md#fanout-aggregate) | warn | An aggregate over rows a one-to-many join repeats |
| [`chasm-join`](../reference/rules.md#chasm-join) | warn | One table joined one-to-many to two others |
| [`unproven-join`](../reference/rules.md#unproven-join) | ignore | A join in an aggregating query that matches no key |
| [`entity-mismatch`](../reference/rules.md#entity-mismatch) | ignore | A comparison between keys of different tables |

All four need a [schema](schema-inference.md) with keys and are judged in the
`postgres`, `mysql` and `sqlite` dialects.

## Totals a join inflates

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

`SUM(i.amount)` is right: each item joins one order and one customer.
`MIN`, `MAX` and `DISTINCT` aggregates such as `COUNT(DISTINCT o.id)` give the
same result over repeated rows and are never reported. To fix the average,
aggregate the items per order first, then join one row per order:

```python
from decimal import Decimal

from sqlty import SQL

# error:
query = SQL[tuple[str, Decimal, Decimal]]("""--sql
WITH items AS (
    SELECT order_id, SUM(amount) AS amount FROM order_items GROUP BY order_id
)
SELECT c.segment, AVG(o.total), SUM(i.amount)
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN items i ON i.order_id = o.id
GROUP BY c.segment""")
```

The `GROUP BY` of a CTE or derived table is its key, so sqlty knows `items` has
one row per order.

## Two one-to-many branches

Joining an order to its items and to its payments repeats each item once per
payment and each payment once per item. No grouping undoes it:

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

Aggregate each branch in its own CTE, one row per order, then join them.

## Joins sqlty cannot prove

sqlty only reports what keys prove. A join on columns that are no primary or
unique key on either side may repeat rows or not, and stays silent unless you
turn on `unproven-join`. Turn it on once the schema declares its keys; in a
schema without them, every join of an aggregating query would be reported.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unproven-join = "warn"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, region TEXT NOT NULL, total NUMERIC NOT NULL);
CREATE TABLE targets (region TEXT NOT NULL, amount NUMERIC NOT NULL);
```

```python
import decimal

from sqlty import SQL

# error: [unproven-join] "the join of orders and targets matches no primary or unique key, so how often it repeats their rows is unknown"
query = SQL[tuple[decimal.Decimal | None]]("""--sql
SELECT SUM(o.total) FROM orders o JOIN targets t ON t.region = o.region""")
```

Declare the key the data has, `UNIQUE (region)` on `targets`, and the join is
to-one.

## Joins on the wrong key

Every id is an integer, so a join of `products.id` to `orders.customer_id`
type-checks and returns rows: products whose id happens to match a customer's.
`entity-mismatch` follows foreign keys to the table whose keys a column holds
and reports a comparison between keys of different tables. It is off by
default, since a polymorphic `owner_id` or a schema without foreign keys can
compare unrelated keys on purpose:

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
entity-mismatch = "warn"
```

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers);
```

```python
from sqlty import SQL

# error: [entity-mismatch] "products.id holds products keys, orders.customer_id holds customers keys"
query = SQL[tuple[str, int]]("""--sql
SELECT p.name, o.id
FROM orders o
JOIN products p ON p.id = o.customer_id""")
```

Comparisons are read in `ON`, `USING`, `WHERE` and `HAVING`. Views, CTEs and
derived tables don't carry entities.

## What sqlty reads from the schema

- A join is to-one toward a table when it equates, or fixes to a constant,
  every column of one of the table's keys: the primary key, a `UNIQUE`
  constraint, or a unique index without `WHERE`.
- A foreign key column holds keys of the column it references, followed to the
  end of the chain.
- A key the schema doesn't declare is unknown. Declaring it, even only in the
  schema file sqlty reads, makes the checks precise.

To fail CI on inflated totals, make the warnings errors:

```toml
[tool.sqlty.rules]
fanout-aggregate = "error"
chasm-join = "error"
```

Try them in the [playground](../playground.md): pick **Joins and Aggregates**
in Examples.

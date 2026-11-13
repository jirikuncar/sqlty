# fanout-aggregate, chasm-join and unproven-join

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unproven-join = "warn"
stale-type = "ignore"
narrow-type = "ignore"
nullable-mismatch = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY, segment TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers, code TEXT, total NUMERIC NOT NULL);
CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
CREATE TABLE payments (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
CREATE TABLE tags (order_id INTEGER NOT NULL REFERENCES orders, name TEXT NOT NULL);
CREATE TABLE codes (code TEXT UNIQUE, label TEXT);
CREATE TABLE indexed (code TEXT, label TEXT);
CREATE UNIQUE INDEX indexed_code ON indexed (code);
CREATE TABLE partial (code TEXT, label TEXT);
CREATE UNIQUE INDEX partial_code ON partial (code) WHERE label IS NOT NULL;
CREATE TABLE dropped (code TEXT, label TEXT, CONSTRAINT dropped_code UNIQUE (code));
ALTER TABLE dropped DROP CONSTRAINT dropped_code;
CREATE TABLE added (code TEXT, label TEXT);
ALTER TABLE added ADD CONSTRAINT added_code UNIQUE (code);
CREATE TABLE rates (code TEXT, day INTEGER, rate NUMERIC, PRIMARY KEY (code, day));
CREATE VIEW code_labels AS SELECT code, label FROM codes;
```

## Fan-out

A parent's column aggregated over a join to its children; the children's
columns are not repeated.

```python
from sqlty import SQL

# error: [fanout-aggregate] "AVG(o.total) aggregates orders, whose rows repeat once per order_items row"
a = SQL[tuple[str, float, float]]("--sql\nSELECT c.segment, AVG(o.total), SUM(i.amount) FROM orders o JOIN customers c ON c.id = o.customer_id JOIN order_items i ON i.order_id = o.id GROUP BY c.segment")
# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per order_items row"
b = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o LEFT JOIN order_items i ON i.order_id = o.id")
# error: [fanout-aggregate] "SUM(total) aggregates orders, whose rows repeat once per order_items row"
c = SQL[tuple[float]]("--sql\nSELECT SUM(total) FROM orders, order_items WHERE order_id = orders.id")
# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per order_items row"
d = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN order_items i ON i.order_id = o.id GROUP BY o.id HAVING SUM(o.total) > 1")
# error: [fanout-aggregate] "COUNT(o.code) aggregates orders, whose rows repeat once per order_items row"
e = SQL[tuple[int]]("--sql\nSELECT COUNT(o.code) FROM orders o JOIN order_items i ON i.order_id = o.id AND i.amount > 0")
```

A table without a key is the many side of a join to its parent's key:

```python
from sqlty import SQL

# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per tags row"
q = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN tags t ON t.order_id = o.id")
```

## Duplicate-safe aggregates

```python
from sqlty import SQL

# error:
a = SQL[tuple[float, int, int]]("--sql\nSELECT MAX(o.total), COUNT(DISTINCT o.id), COUNT(*) FROM orders o JOIN order_items i ON i.order_id = o.id")
b = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) OVER () FROM orders o JOIN order_items i ON i.order_id = o.id")
c = SQL[tuple[int, float]]("--sql\nSELECT o.id, (SELECT SUM(total) FROM orders) FROM orders o JOIN order_items i ON i.order_id = o.id")
d = SQL[tuple[float]]("--sql\nSELECT SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id")
```

### `AVG` grouped by the repeated table's key

Each group holds copies of one order, so `AVG(o.total)` is `o.total`. `SUM`
and `COUNT` still count every copy, and `AVG` grouped by another column still
mixes orders with different numbers of items.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int, float, float]]("--sql\nSELECT o.id, AVG(o.total), SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id GROUP BY o.id")
b = SQL[tuple[int, float]]("--sql\nSELECT i.order_id, AVG(o.total) FROM orders o JOIN order_items i ON i.order_id = o.id GROUP BY i.order_id")
```

```python
from sqlty import SQL

# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per order_items row"
a = SQL[tuple[int, float]]("--sql\nSELECT o.id, SUM(o.total) FROM orders o JOIN order_items i ON i.order_id = o.id GROUP BY o.id")
# error: [fanout-aggregate] "AVG(o.total) aggregates orders, whose rows repeat once per order_items row"
b = SQL[tuple[int, float]]("--sql\nSELECT o.customer_id, AVG(o.total) FROM orders o JOIN order_items i ON i.order_id = o.id GROUP BY o.customer_id")
```

## Aggregated first

A CTE or derived table grouped by the join column, or selected `DISTINCT`, has
one row per order.

```python
from sqlty import SQL

# error:
a = SQL[tuple[str, float, float]]("--sql\nWITH t AS (SELECT order_id, SUM(amount) AS amount FROM order_items GROUP BY order_id) SELECT c.segment, AVG(o.total), SUM(t.amount) FROM orders o JOIN customers c ON c.id = o.customer_id JOIN t ON t.order_id = o.id GROUP BY c.segment")
b = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN (SELECT DISTINCT order_id FROM order_items) i ON i.order_id = o.id")
c = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN (SELECT order_id AS id, COUNT(*) AS n FROM order_items GROUP BY 1) i ON i.id = o.id")
d = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o CROSS JOIN (SELECT MAX(amount) AS top FROM order_items) m")
```

A CTE that selects a table's key keeps it:

```python
from sqlty import SQL

# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per big row"
a = SQL[tuple[float]]("--sql\nWITH big AS (SELECT id, order_id FROM order_items WHERE amount > 10) SELECT SUM(o.total) FROM orders o JOIN big ON big.order_id = o.id")
# error:
b = SQL[tuple[float]]("--sql\nWITH big AS (SELECT id, total FROM orders WHERE total > 10) SELECT SUM(o.total) FROM orders o JOIN big ON big.id = o.id")
```

## Keys

`UNIQUE` columns and constraints, unique indexes without `WHERE`, and composite
keys fixed by a constant are keys; a partial index, a dropped constraint and a
view's columns are not.

```python
from sqlty import SQL

# error:
a = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN codes k ON k.code = o.code")
b = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN indexed k ON k.code = o.code")
c = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN added k ON k.code = o.code")
d = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN rates r ON r.code = o.code AND r.day = 3")
```

```python
from sqlty import SQL

# error: [unproven-join] "the join of orders and partial matches no primary or unique key"
a = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN partial k ON k.code = o.code")
# error: [unproven-join] "the join of orders and dropped matches no primary or unique key"
b = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN dropped k ON k.code = o.code")
# error: [unproven-join] "the join of orders and rates matches no primary or unique key"
c = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN rates r ON r.code = o.code")
# error: [unproven-join] "the join of orders and code_labels matches no primary or unique key"
d = SQL[tuple[float]]("--sql\nSELECT SUM(o.total) FROM orders o JOIN code_labels v ON v.code = o.code")
```

A self-join is named by its aliases:

```python
from sqlty import SQL

# error: [unproven-join] "the join of a and b matches no primary or unique key"
q = SQL[tuple[float]]("--sql\nSELECT SUM(a.total) FROM orders a JOIN orders b ON b.code = a.code")
```

A scalar subquery fixes a key as a constant does, when it reads no column of the joined
table: the latest item of each order, here. Its columns must be qualified.

```python
from sqlty import SQL

# error:
on = SQL[tuple[float]](
    "--sql\nSELECT SUM(o.total) FROM orders o JOIN order_items i ON i.order_id = o.id"
    " AND i.id = (SELECT max(i2.id) FROM order_items i2 WHERE i2.order_id = o.id)"
)
where = SQL[tuple[float]](
    "--sql\nSELECT SUM(o.total) FROM orders o JOIN order_items i ON i.order_id = o.id"
    " WHERE (SELECT max(i2.id) FROM order_items i2 WHERE i2.order_id = o.id) = i.id"
)
```

```python
from sqlty import SQL

# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per order_items row"
own = SQL[tuple[float]](
    "--sql\nSELECT SUM(o.total) FROM orders o JOIN order_items i ON i.order_id = o.id"
    " AND i.id = (SELECT max(i2.id) FROM order_items i2 WHERE i2.order_id = i.order_id)"
)
# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per order_items row"
unqualified = SQL[tuple[float]](
    "--sql\nSELECT SUM(o.total) FROM orders o JOIN order_items i ON i.order_id = o.id"
    " AND i.id = (SELECT max(id) FROM order_items i2 WHERE i2.order_id = o.id)"
)
```

## Chasm

```python
from sqlty import SQL

# error: [chasm-join] "orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"
# error: [fanout-aggregate] "SUM(i.amount) aggregates order_items, whose rows repeat once per payments row"
# error: [fanout-aggregate] "SUM(p.amount) aggregates payments, whose rows repeat once per order_items row"
q = SQL[tuple[int, float, float]]("--sql\nSELECT o.id, SUM(i.amount), SUM(p.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id")
```

## Not aggregating

```python
from sqlty import SQL

# error:
q = SQL[tuple[int, float]]("--sql\nSELECT o.id, i.amount FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id")
```

## TPC-H

```toml
[tool.sqlty]
schema = "tpch.sql"

[tool.sqlty.rules]
stale-type = "ignore"
narrow-type = "ignore"
nullable-mismatch = "ignore"
```

`tpch.sql`:

```sql
CREATE TABLE customer (c_custkey INTEGER PRIMARY KEY, c_name TEXT NOT NULL, c_mktsegment TEXT NOT NULL);
CREATE TABLE orders (o_orderkey INTEGER PRIMARY KEY, o_custkey INTEGER NOT NULL REFERENCES customer, o_totalprice NUMERIC NOT NULL, o_orderdate DATE NOT NULL);
CREATE TABLE lineitem (
    l_orderkey INTEGER NOT NULL REFERENCES orders,
    l_linenumber INTEGER NOT NULL,
    l_extendedprice NUMERIC NOT NULL,
    l_discount NUMERIC NOT NULL,
    PRIMARY KEY (l_orderkey, l_linenumber)
);
```

Q3 sums line items per order, which repeats nothing:

```python
from sqlty import SQL

# error:
q3 = SQL[tuple[int, float]]("""--sql
SELECT l_orderkey, SUM(l_extendedprice * (1 - l_discount)) AS revenue
FROM customer, orders, lineitem
WHERE c_mktsegment = 'BUILDING'
  AND c_custkey = o_custkey
  AND l_orderkey = o_orderkey
GROUP BY l_orderkey""")
```

Summing the order total next to its line items counts it once per line:

```python
from sqlty import SQL

# error: [fanout-aggregate] "SUM(o_totalprice) aggregates orders, whose rows repeat once per lineitem row"
q = SQL[tuple[str, float, float]]("""--sql
SELECT c_mktsegment, SUM(o_totalprice), SUM(l_extendedprice)
FROM customer
JOIN orders ON o_custkey = c_custkey
JOIN lineitem ON l_orderkey = o_orderkey
GROUP BY c_mktsegment""")
```

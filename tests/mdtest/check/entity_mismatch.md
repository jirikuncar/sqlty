# entity-mismatch

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
entity-mismatch = "warn"
stale-type = "ignore"
narrow-type = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers, total INTEGER NOT NULL);
CREATE TABLE order_items (
    order_id INTEGER NOT NULL REFERENCES orders,
    product_id INTEGER NOT NULL REFERENCES products (id),
    PRIMARY KEY (order_id, product_id)
);
CREATE TABLE vip (customer_id INTEGER PRIMARY KEY REFERENCES customers);
CREATE TABLE employees (id INTEGER PRIMARY KEY, manager_id INTEGER REFERENCES employees);
```

## Different entities

```python
from sqlty import SQL

# error: [entity-mismatch] "products.id holds products keys, orders.customer_id holds customers keys"
a = SQL[tuple[str]]("--sql\nSELECT p.name FROM orders o JOIN products p ON p.id = o.customer_id")
# error: [entity-mismatch] "order_items.product_id holds products keys, orders.id holds orders keys"
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o, order_items i WHERE i.product_id = o.id")
# error: [entity-mismatch] "orders.customer_id holds customers keys, orders.id holds orders keys"
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.total = 3 OR o.customer_id <> o.id")
# error: [entity-mismatch] "products.id holds products keys, orders.customer_id holds customers keys"
d = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN products p ON p.id IN (o.customer_id, 3)")
# error: [entity-mismatch] "orders.customer_id holds customers keys, products.id holds products keys"
e = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id IN (SELECT id FROM products)")
# error: [entity-mismatch] "order_items.order_id holds orders keys, vip.customer_id holds customers keys"
f = SQL[tuple[int]]("--sql\nSELECT 1 FROM vip v JOIN order_items i ON i.order_id = v.customer_id")
```

Two columns of one composite key are told apart by their column:

```python
from sqlty import SQL

# error: [entity-mismatch] "order_items.order_id holds orders keys, order_items.product_id holds products keys"
q = SQL[tuple[int]]("--sql\nSELECT 1 FROM order_items a JOIN order_items b ON a.order_id = b.product_id")
```

## Same entity

A foreign key holds keys of the column it references, through any number of
foreign keys, a self-reference included. `USING` compares the named columns.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON c.id = o.customer_id")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id IN (SELECT customer_id FROM vip)")
c = SQL[tuple[int]]("--sql\nSELECT 1 FROM vip v JOIN orders o ON o.customer_id = v.customer_id")
d = SQL[tuple[int]]("--sql\nSELECT e.id FROM employees e JOIN employees m ON m.id = e.manager_id")
e = SQL[tuple[int]]("--sql\nSELECT v.customer_id FROM vip v JOIN orders USING (customer_id)")
f = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN order_items i ON i.order_id = o.id")
```

## Not judged

Columns that are no key, constants, CTEs and derived tables carry no entity.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.total = o.id")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id = 3")
c = SQL[tuple[int]]("--sql\nWITH orders AS (SELECT 1 AS id) SELECT 1 FROM orders o JOIN products p ON p.id = o.id")
d = SQL[tuple[int]]("--sql\nSELECT 1 FROM (SELECT customer_id AS id FROM orders) o JOIN products p ON p.id = o.id")
```

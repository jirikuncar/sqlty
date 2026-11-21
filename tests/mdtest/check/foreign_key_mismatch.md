# foreign-key-mismatch

A column with a `FOREIGN KEY` may be compared (`=`, `<>`, `IN`, `USING`, in `ON`, `WHERE`,
`HAVING`) only with:

- the same column, of the same or another row (self-join, other alias),
- the column its foreign key references,
- a column holding the same keys: one referencing the same column, or reaching it
  through a chain of foreign keys, or
- a column of the same name that is no key.

The columns of a composite foreign key match pairwise and together: each column with its
own referenced column, all of them against one row (one alias) of the referenced table.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
stale-type = "ignore"
narrow-type = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customers (id),
    total INTEGER NOT NULL
);
CREATE TABLE vip (customer_id INTEGER PRIMARY KEY REFERENCES customers);
CREATE TABLE vip_notes (vip_id INTEGER NOT NULL REFERENCES vip (customer_id), note TEXT);
CREATE TABLE employees (id INTEGER PRIMARY KEY, manager_id INTEGER REFERENCES employees (id));

CREATE TABLE warehouses (region TEXT, code TEXT, PRIMARY KEY (region, code));
CREATE TABLE stock (
    region TEXT NOT NULL,
    code TEXT NOT NULL,
    product_id INTEGER NOT NULL REFERENCES products,
    FOREIGN KEY (region, code) REFERENCES warehouses (region, code)
);
CREATE TABLE shipments (
    id INTEGER PRIMARY KEY,
    from_region TEXT NOT NULL,
    from_code TEXT NOT NULL,
    to_region TEXT NOT NULL,
    to_code TEXT NOT NULL,
    FOREIGN KEY (from_region, from_code) REFERENCES warehouses (region, code),
    FOREIGN KEY (to_region, to_code) REFERENCES warehouses (region, code)
);
```

## Single-column keys

### Allowed

The referenced column, either operand order; the same column of another row.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON c.id = o.customer_id")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON o.customer_id = c.id")
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o, customers c WHERE c.id = o.customer_id")
d = SQL[tuple[int]]("--sql\nSELECT a.id FROM orders a JOIN orders b ON a.customer_id = b.customer_id AND a.id <> b.id")
e = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id IN (SELECT id FROM customers)")
f = SQL[tuple[int]]("--sql\nSELECT v.customer_id FROM vip v JOIN customers c ON c.id = v.customer_id")
g = SQL[tuple[int]]("--sql\nSELECT n.vip_id FROM vip_notes n JOIN vip v ON v.customer_id = n.vip_id")
```

Self-reference: the manager's id, another employee's manager, and the row's own id.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT e.id FROM employees e JOIN employees m ON m.id = e.manager_id")
b = SQL[tuple[int]]("--sql\nSELECT e.id FROM employees e JOIN employees p ON p.manager_id = e.manager_id")
c = SQL[tuple[int]]("--sql\nSELECT e.id FROM employees e WHERE e.manager_id = e.id")
```

Two foreign keys referencing the same column (siblings), or reaching it through a
chain of foreign keys:

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN vip v ON v.customer_id = o.customer_id")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN vip v USING (customer_id)")
c = SQL[tuple[int]]("--sql\nSELECT n.vip_id FROM vip_notes n JOIN customers c ON c.id = n.vip_id")
d = SQL[tuple[int]]("--sql\nSELECT n.vip_id FROM vip_notes n JOIN orders o ON o.customer_id = n.vip_id")
```

### Reported

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
a = SQL[tuple[str]]("--sql\nSELECT p.name FROM orders o JOIN products p ON p.id = o.customer_id")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
b = SQL[tuple[str]]("--sql\nSELECT p.name FROM orders o JOIN products p ON o.customer_id = p.id")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not orders.id"
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id <> o.id")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not orders.total"
d = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id = o.total")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
e = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN products p ON p.id IN (o.customer_id, 3)")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
f = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id IN (SELECT id FROM products)")
# error: [foreign-key-mismatch] "employees.manager_id references employees.id, not orders.id"
g = SQL[tuple[int]]("--sql\nSELECT 1 FROM employees e JOIN orders o ON o.id = e.manager_id")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
h = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN products p ON NOT (o.customer_id = p.id)")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
i = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o, products p WHERE o.total > 1 OR o.customer_id = p.id")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
k = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o, products p WHERE NOT (o.customer_id IN (p.id, 3))")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
j = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN products p ON o.id = p.id GROUP BY o.id, o.customer_id, p.id HAVING o.customer_id = p.id")
```

Both sides hold a foreign key: each side is judged, the first failing one is reported.

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock.product_id references products.id, not orders.customer_id"
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s JOIN orders o ON s.product_id = o.customer_id")
```

### In subqueries of conditions

A subquery of a condition is judged in its own scope, which sees the outer one.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE EXISTS (SELECT 1 FROM customers c WHERE c.id = o.customer_id)")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE EXISTS (SELECT 1 FROM customers c WHERE c.id IN (SELECT customer_id FROM orders))")
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE EXISTS (SELECT 1 FROM products p WHERE p.id = o.customer_id)")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.id IN (SELECT o2.id FROM orders o2 JOIN products p ON p.id = o2.customer_id)")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
c = SQL[tuple[int]]("--sql\nSELECT 1 WHERE EXISTS (WITH c AS (SELECT 1 FROM orders o JOIN products p ON p.id = o.customer_id) SELECT 1 FROM c)")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
d = SQL[tuple[int]]("--sql\nSELECT 1 WHERE 1 IN (SELECT 2 UNION SELECT 1 FROM orders o JOIN products p ON p.id = o.customer_id)")
```

### A namesake that is no key

A column of the same name that is neither a key nor references one, as in a log or
audit table, holds the same keys. `USING` reads the nearest source before the
joined table that has the column.

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY);
CREATE TABLE products (id INTEGER PRIMARY KEY);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id));
CREATE TABLE reviews (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES products (id));
CREATE TABLE events (id BIGINT, customer_id INTEGER, kind TEXT);
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN events e ON e.customer_id = o.customer_id")
b = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN events e USING (customer_id)")
c = SQL[tuple[int]]("--sql\nWITH d AS (SELECT 1 AS customer_id) SELECT 1 FROM reviews r, d JOIN orders o USING (customer_id)")
d = SQL[tuple[int]]("--sql\nSELECT 1 FROM reviews r, (SELECT 1 AS customer_id) d JOIN orders o USING (customer_id)")
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not events.id"
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN events e ON e.id = o.customer_id")
# error: [foreign-key-mismatch] "reviews.customer_id references products.id, not orders.customer_id"
b = SQL[tuple[int]]("--sql\nSELECT 1 FROM reviews r JOIN orders o USING (customer_id)")
```

## Composite keys

### Allowed

Every column with its own referenced column, of one row, in any order, with `AND` and any
other conditions; `USING`; the same key of another row.

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND w.code = s.code")
b = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON s.code = w.code AND s.region = w.region AND s.product_id = 3")
c = SQL[tuple[str]]("--sql\nSELECT code FROM stock JOIN warehouses USING (region, code)")
d = SQL[tuple[str]]("--sql\nSELECT a.code FROM stock a JOIN stock b ON a.region = b.region AND a.code = b.code")
e = SQL[tuple[int]]("--sql\nSELECT s.id FROM shipments s JOIN warehouses w ON w.region = s.to_region AND w.code = s.to_code")
f = SQL[tuple[int]]("--sql\nSELECT s.id FROM shipments s, warehouses w WHERE (s.from_region, s.from_code) = (w.region, w.code)")
g = SQL[tuple[int]]("--sql\nSELECT s.id FROM shipments s WHERE (s.from_region, s.from_code) IN (SELECT region, code FROM warehouses)")
```

An `OR` branch or `IN` list item is read with the terms `AND` joins it to:

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND (w.code = s.code OR s.code IS NULL)")
b = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON (w.region, w.code) = (s.region, s.code) WHERE s.region = w.region OR s.product_id = 2")
```

A key with a column `IS NULL` references no row:

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND s.code IS NULL")
```

A key column and its referenced column equal to the same constant or parameter, as
a tenant is:

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region = $1 WHERE s.region = $1")
b = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region = 'eu' AND 'eu' = s.region")
```

A single-item `IN` list pins as `=` does:

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region IN ('eu') AND s.region IN ('eu')")
```

Equal through other columns, constants or parameters, as a tenant compared with a
third table's is; `IS NOT DISTINCT FROM` reads as `=`, and a cast column as itself:

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN stock t ON t.region = s.region JOIN warehouses w ON w.region = t.region AND w.code = s.code")
b = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region = :r WHERE :r = s.region")
c = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region IS NOT DISTINCT FROM s.region AND w.code = s.code")
d = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region::text = s.region AND w.code = s.code::text")
e = SQL[tuple[str]]("--sql\nSELECT s.code FROM stock s WHERE s.code IN (SELECT w.code FROM warehouses w WHERE w.region = s.region)")
f = SQL[tuple[str]]("--sql\nSELECT s.code FROM stock s WHERE s.code IN (SELECT w.code FROM warehouses w WHERE w.region IS NOT DISTINCT FROM s.region)")
g = SQL[tuple[str]]("--sql\nSELECT s.code FROM stock s WHERE s.region = 'eu' AND s.code IN (SELECT code FROM warehouses WHERE 'eu' = region)")
```

A term read as no comparison that relates both rows may compare the rest of the key:

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND lower(w.code) = lower(s.code)")
b = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND COALESCE(w.code = s.code, false)")
c = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON CASE WHEN s.code = w.code THEN w.region = s.region END")
```

### Reported

Columns swapped:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock.region references warehouses.region, not warehouses.code"
# error: [foreign-key-mismatch] "stock.code references warehouses.code, not warehouses.region"
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.region AND w.region = s.code")
```

Pairs right, rows different:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock (region, code) references one warehouses row, compared with a.region and b.code"
a = SQL[tuple[str]]("--sql\nSELECT a.code FROM stock s, warehouses a, warehouses b WHERE s.region = a.region AND s.code = b.code")
```

Columns of two foreign keys mixed:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "shipments (from_region, from_code) and (to_region, to_code) mixed: from_region = w.region, to_code = w.code"
a = SQL[tuple[int]]("--sql\nSELECT s.id FROM shipments s JOIN warehouses w ON w.region = s.from_region AND w.code = s.to_code")
```

Only part of the key compared, the rest pinned on one side only:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region")
# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on code only"
b = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region OR w.code = s.code")
# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
c = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND w.code = $1")
# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
e = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND w.code IN (s.code, 'x')")
# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
d = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region AND w.code <> 'x' AND s.code <> 'x'")
```

Pinned to different constants or parameters, or to `NULL`:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on code only"
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region = $1 WHERE s.region = $2")
# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on code only"
b = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region = 'eu' AND s.region = 'us'")
# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on code only"
c = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region = NULL AND s.region = NULL")
```

A correlated subquery compares the rest of the key only when it selects it for `IN`:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE EXISTS (SELECT 1 FROM warehouses w WHERE w.region = s.region)")
```

### A self-referencing composite key

`schema.sql`:

```sql
CREATE TABLE nodes (tree INT, id INT, parent_id INT, PRIMARY KEY (tree, id), FOREIGN KEY (tree, parent_id) REFERENCES nodes (tree, id));
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM nodes n JOIN nodes p ON p.tree = n.tree AND p.id = n.parent_id")
b = SQL[tuple[int]]("--sql\nSELECT 1 FROM nodes n JOIN nodes p ON (p.tree, p.id) = (n.tree, n.parent_id)")
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "nodes (tree, parent_id) references nodes (tree, id), compared with p on parent_id only"
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM nodes n JOIN nodes p ON p.id = n.parent_id")
```

## Keys sharing a tenant column

Every key of a multi-tenant schema starts with the tenant. `t.tenant_id = u.tenant_id`
compares that part of each key, and a complete key compared elsewhere ties the rows.

`schema.sql`:

```sql
CREATE TABLE users (tenant_id INTEGER, id INTEGER, PRIMARY KEY (tenant_id, id));
CREATE TABLE projects (
    tenant_id INTEGER, id INTEGER, owner_id INTEGER,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, owner_id) REFERENCES users (tenant_id, id)
);
CREATE TABLE tasks (
    tenant_id INTEGER, id INTEGER, project_id INTEGER, assignee_id INTEGER,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, assignee_id) REFERENCES users (tenant_id, id)
);
```

### Allowed

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT t.id FROM tasks t JOIN projects p ON p.tenant_id = t.tenant_id AND p.id = t.project_id JOIN users u ON u.tenant_id = p.tenant_id AND u.id = p.owner_id")
b = SQL[tuple[int]]("--sql\nSELECT t.id FROM tasks t JOIN projects p ON p.tenant_id = t.tenant_id AND p.id = t.project_id JOIN users u ON u.tenant_id = t.tenant_id AND u.id = p.owner_id")
c = SQL[tuple[int]]("--sql\nSELECT t.id FROM tasks t WHERE t.assignee_id IN (SELECT u.id FROM users u WHERE u.tenant_id = t.tenant_id)")
d = SQL[tuple[int]]("--sql\nSELECT t.id FROM tasks t WHERE t.tenant_id = $1 AND t.assignee_id IN (SELECT id FROM users WHERE tenant_id = $1)")
e = SQL[tuple[int]]("--sql\nSELECT t.id FROM tasks t JOIN projects p ON p.tenant_id = t.tenant_id AND t.assignee_id = p.owner_id")
```

### Reported

The rest of the key against no row, or a `?` written twice, which binds two parameters:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "tasks (tenant_id, assignee_id) references users (tenant_id, id), compared with u on assignee_id only"
a = SQL[tuple[int]]("--sql\nSELECT t.id FROM tasks t JOIN users u ON u.id = t.assignee_id")
# error: [foreign-key-mismatch] "tasks (tenant_id, assignee_id) references users (tenant_id, id), compared with users on assignee_id only"
b = SQL[tuple[int]]("--sql\nSELECT t.id FROM tasks t WHERE t.tenant_id = ? AND t.assignee_id IN (SELECT id FROM users WHERE tenant_id = ?)")
```

## Tuples with `IN`

Row values pair up by position, in `IN` lists and in `IN` subqueries, `NOT IN` too.

### Allowed

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE (s.region, s.code) IN (SELECT region, code FROM warehouses)")
b = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE (s.region, s.code) NOT IN (SELECT w.region, w.code FROM warehouses w)")
c = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s JOIN warehouses w ON (s.region, s.code) IN ((w.region, w.code), ('eu', 'a'))")
d = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE (s.region, s.code, s.product_id) IN (SELECT region, code, 1 FROM warehouses)")
e = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE (o.customer_id, o.total) IN (SELECT id, 10 FROM customers)")
f = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE (o.customer_id, o.id) IN (SELECT customer_id, id FROM orders)")
g = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE (o.customer_id) IN (SELECT id FROM customers)")
h = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE (s.region, s.code) IN (('eu', 'a'), ('us', 'b'))")
i = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s, warehouses w WHERE (s.region, s.code) IN (SELECT w.region, w.code FROM products)")
```

Siblings: both keys reference `warehouses (region, code)`.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM shipments s WHERE (s.from_region, s.from_code) IN (SELECT to_region, to_code FROM shipments)")
```

### Reported

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock.region references warehouses.region, not warehouses.code"
# error: [foreign-key-mismatch] "stock.code references warehouses.code, not warehouses.region"
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE (s.region, s.code) IN (SELECT code, region FROM warehouses)")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE (o.customer_id, o.total) IN (SELECT id, 10 FROM products)")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o, products p WHERE (o.id, o.customer_id) NOT IN ((1, p.id))")
# error: [foreign-key-mismatch] "stock (region, code) references one warehouses row, compared with a.region and b.code"
d = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s, warehouses a, warehouses b WHERE (s.region, s.code) IN ((a.region, b.code))")
# error: [foreign-key-mismatch] "shipments (from_region, from_code) and (to_region, to_code) mixed: from_region = warehouses.region, to_code = warehouses.code"
e = SQL[tuple[int]]("--sql\nSELECT s.id FROM shipments s WHERE (s.from_region, s.to_code) IN (SELECT region, code FROM warehouses)")
```

Only part of the key in the tuple:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with warehouses on region only"
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE (s.region, s.product_id) IN (SELECT region, 1 FROM warehouses)")
```

## Not judged

Neither side has a foreign key, constants, parameters, CTEs and derived tables.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN products p ON p.id = o.id")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id = 3")
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id = :customer")
d = SQL[tuple[int]]("--sql\nWITH x AS (SELECT customer_id FROM orders) SELECT 1 FROM x JOIN products p ON p.id = x.customer_id")
e = SQL[tuple[int]]("--sql\nSELECT 1 FROM (SELECT customer_id FROM orders) o JOIN products p ON p.id = o.customer_id")
f = SQL[tuple[int]]("--sql\nSELECT s.id FROM shipments s WHERE s.from_region = 'eu'")
g = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id IN (SELECT id FROM products UNION SELECT id FROM customers)")
h = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.customer_id IN (SELECT * FROM products)")
```

Neither are row values of different lengths, which the database rejects.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s, warehouses w WHERE (s.region, s.code) = (w.region)")
b = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s, warehouses w WHERE (s.region, s.code) = (w.region, w.code, 1)")
c = SQL[tuple[int]]("--sql\nSELECT 1 FROM stock s WHERE (s.region, s.code) IN (SELECT region FROM warehouses)")
```

## Tables defined later

A key may reference a table created further down: in a column, a table constraint,
`ALTER TABLE` or a cycle. Schema files kept in name order do so, and so do `mysqldump`
(behind `FOREIGN_KEY_CHECKS = 0`) and SQLite, which checks keys only when rows change.

`schema.sql`:

```sql
CREATE TABLE rotation_state (rotation_id INTEGER PRIMARY KEY, FOREIGN KEY (rotation_id) REFERENCES rotations (id));
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers, product_id INTEGER);
ALTER TABLE orders ADD CONSTRAINT orders_product_id_fkey FOREIGN KEY (product_id) REFERENCES products (id);
CREATE TABLE customers (id INTEGER PRIMARY KEY, last_order_id INTEGER REFERENCES orders (id));
CREATE TABLE products (id INTEGER PRIMARY KEY);
CREATE TABLE rotations (id INTEGER PRIMARY KEY);
CREATE TABLE schedule_rules (id INTEGER PRIMARY KEY, rotation_id INTEGER REFERENCES rotations (id));
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT r.id FROM schedule_rules r JOIN rotation_state s ON s.rotation_id = r.rotation_id")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON c.id = o.customer_id")
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.product_id")
d = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON c.last_order_id = o.id")
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "rotation_state.rotation_id references rotations.id, not schedule_rules.id"
a = SQL[tuple[int]]("--sql\nSELECT r.id FROM schedule_rules r JOIN rotation_state s ON s.rotation_id = r.id")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.customer_id")
# error: [foreign-key-mismatch] "orders.product_id references products.id, not customers.id"
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON c.id = o.product_id")
# error: [foreign-key-mismatch] "customers.last_order_id references orders.id, not products.id"
d = SQL[tuple[int]]("--sql\nSELECT c.id FROM customers c JOIN products p ON p.id = c.last_order_id")
```

### MySQL: a dump in name order

`schema.sql`:

```sql
SET FOREIGN_KEY_CHECKS = 0;
CREATE TABLE orders (id INT PRIMARY KEY, customer_id INT, FOREIGN KEY (customer_id) REFERENCES customers (id)) ENGINE = InnoDB;
CREATE TABLE customers (id INT PRIMARY KEY) ENGINE = InnoDB;
CREATE TABLE products (id INT PRIMARY KEY) ENGINE = InnoDB;
SET FOREIGN_KEY_CHECKS = 1;
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"

[tool.sqlty.rules]
stale-type = "ignore"
narrow-type = "ignore"
```

```python
from sqlty import SQL

a = SQL[tuple[int]]("-- sql\nSELECT o.id FROM orders o JOIN customers c ON c.id = o.customer_id")
# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
b = SQL[tuple[int]]("-- sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.customer_id")
```

## Renamed tables and columns

A key whose referenced table or column was renamed is not judged: the catalog
keeps the old name.

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY);
CREATE TABLE products (id INTEGER PRIMARY KEY);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), product_id INTEGER REFERENCES products (id));
ALTER TABLE customers RENAME COLUMN id TO cid;
ALTER TABLE products RENAME TO items;
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN customers c ON c.cid = o.customer_id")
b = SQL[tuple[int]]("--sql\nSELECT 1 FROM orders o JOIN items i ON i.id = o.product_id")
```

### Referencing columns dropped, added or renamed

A dropped column takes its keys along, a renamed one keeps them, and
`ADD COLUMN ... REFERENCES` declares one.

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY);
CREATE TABLE products (id INTEGER PRIMARY KEY);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), buyer INTEGER REFERENCES customers (id));
ALTER TABLE orders DROP COLUMN customer_id;
ALTER TABLE orders ADD COLUMN customer_id INTEGER REFERENCES products (id);
ALTER TABLE orders RENAME COLUMN buyer TO buyer_id;
ALTER TABLE orders ADD COLUMN gone_id INTEGER REFERENCES missing (id);
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.customer_id")
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON c.id = o.buyer_id")
c = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.gone_id")
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "orders.customer_id references products.id, not customers.id"
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN customers c ON c.id = o.customer_id")
# error: [foreign-key-mismatch] "orders.buyer_id references customers.id, not products.id"
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.buyer_id")
```

## Dialects

Foreign keys come from the catalog, so every way a dialect declares one counts.

### PostgreSQL: `ALTER TABLE`, quoted names, `NOT VALID`

`schema.sql`:

```sql
CREATE TABLE "Customers" (id INTEGER PRIMARY KEY);
CREATE TABLE products (id INTEGER PRIMARY KEY);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, product_id INTEGER);
ALTER TABLE orders ADD CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES "Customers" (id);
ALTER TABLE orders ADD CONSTRAINT orders_product FOREIGN KEY (product_id) REFERENCES products (id) NOT VALID;
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
stale-type = "ignore"
narrow-type = "ignore"
```

A `NOT VALID` key is still declared and meant to hold, so it is judged too.

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "orders.customer_id references Customers.id, not products.id"
a = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.customer_id")
# error: [foreign-key-mismatch] "orders.product_id references products.id, not Customers.id"
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN \"Customers\" c ON c.id = o.product_id")
```

### SQLite: `REFERENCES` without a column

The primary key of the referenced table, composite included.

`schema.sql`:

```sql
CREATE TABLE warehouses (region TEXT, code TEXT, PRIMARY KEY (region, code));
CREATE TABLE stock (region TEXT, code TEXT, FOREIGN KEY (region, code) REFERENCES warehouses);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.rules]
stale-type = "ignore"
narrow-type = "ignore"
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock.region references warehouses.region, not warehouses.code"
# error: [foreign-key-mismatch] "stock.code references warehouses.code, not warehouses.region"
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.region AND w.region = s.code")
```

An empty `IN` list, which SQLite accepts, leaves the other terms judged:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.region WHERE s.rowid IN ()")
```

#### Key declared out of column order

The columns of the key pair in the order the primary key lists them, not the order of
the table's columns.

`schema.sql`:

```sql
CREATE TABLE warehouses (region TEXT, code TEXT, PRIMARY KEY (code, region));
CREATE TABLE stock (code TEXT, region TEXT, FOREIGN KEY (code, region) REFERENCES warehouses);
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.code = s.code AND w.region = s.region")
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock.code references warehouses.code, not warehouses.region"
# error: [foreign-key-mismatch] "stock.region references warehouses.region, not warehouses.code"
a = SQL[tuple[str]]("--sql\nSELECT w.code FROM stock s JOIN warehouses w ON w.region = s.code AND w.code = s.region")
```

### MySQL: only InnoDB keys

MySQL parses and ignores the foreign keys of other engines.

`schema.sql`:

```sql
CREATE TABLE customers (id INT PRIMARY KEY) ENGINE = InnoDB;
CREATE TABLE products (id INT PRIMARY KEY) ENGINE = InnoDB;
CREATE TABLE orders (id INT PRIMARY KEY, customer_id INT, FOREIGN KEY (customer_id) REFERENCES customers (id)) ENGINE = InnoDB;
CREATE TABLE archive (id INT PRIMARY KEY, customer_id INT, FOREIGN KEY (customer_id) REFERENCES customers (id)) ENGINE = MyISAM;
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"

[tool.sqlty.rules]
stale-type = "ignore"
narrow-type = "ignore"
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
a = SQL[tuple[int]]("-- sql\nSELECT o.id FROM orders o JOIN products p ON p.id = o.customer_id")
# error:
b = SQL[tuple[int]]("-- sql\nSELECT a.id FROM archive a JOIN products p ON p.id = a.customer_id")
```

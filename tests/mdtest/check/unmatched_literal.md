# Comparing with a value an enum or CHECK list never holds

`unmatched-literal` reports a `WHERE` term that compares a column of known values with
constants it never holds.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TYPE pay_kind AS ENUM ('card', 'iban');
CREATE TABLE orders (
    id SERIAL PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid')),
    level INTEGER CHECK (level IN (1, 2)),
    kind pay_kind
);
CREATE VIEW paid AS SELECT id, status FROM orders WHERE status = 'paid';
```

## SELECT, UPDATE and DELETE

```python
from sqlty import SQL

# error: [unmatched-literal] "`orders.status` is never 'refunded', so this WHERE matches no row"
a = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE status = 'refunded'")
# error: [unmatched-literal] "`o.kind` is never 'cash' or 'check', so this WHERE matches no row"
b = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o WHERE o.id > %s AND o.kind IN ('cash', 'check')")
# error: [unmatched-literal] "`orders.level` is never 3, so this WHERE matches no row"
c = "--sql\nUPDATE orders SET status = 'paid' WHERE 3 = level"
# error: [unmatched-literal] "`orders.status` is never 'refunded', so this WHERE matches no row"
d = "--sql\nDELETE FROM orders WHERE status = 'refunded'::text"
```

## Through a view or a CTE

The values a view or CTE keeps count: `paid` holds only `'paid'` rows.

```python
from sqlty import SQL

# error: [unmatched-literal] "`paid.status` is never 'pending', so this WHERE matches no row"
a = SQL[tuple[int]]("--sql\nSELECT id FROM paid WHERE status = 'pending'")
# error: [unmatched-literal] "`t.status` is never 'pending', so this WHERE matches no row"
b = SQL[tuple[int]]("--sql\nWITH t AS (SELECT id, status FROM orders WHERE status = 'paid') SELECT id FROM t WHERE status = 'pending'")
```

## A recursive CTE

A row the recursive term reads holds a value of the anchor, or one the term made from
such a row. With several anchor branches, each counts.

```python
from sqlty import SQL

# error: [unmatched-literal] "`t.kind` is never 'leaf', so this WHERE matches no row"
a = SQL[tuple[int]]("--sql\nWITH RECURSIVE t(id, kind) AS (SELECT 1, 'root' UNION ALL SELECT id + 1, kind FROM t WHERE kind = 'leaf') SELECT id FROM t")
# error:
b = SQL[tuple[int]]("--sql\nWITH RECURSIVE t(id, kind) AS (SELECT 1, 'root' UNION ALL SELECT 2, 'leaf' UNION ALL SELECT id + 10, kind FROM t WHERE kind = 'leaf' AND id < 50) SELECT id FROM t")
```

## What is not reported

A listed value, a value some constant of an `IN` list matches, a term under `OR` or
`NOT`, a constant of another kind and a parameter are not judged.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE status = 'paid' AND level IN (2, 3)")
b = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE status = 'refunded' OR id = %s")
c = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE NOT (status = 'refunded')")
d = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE level = '3' AND status = %s")
e = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE status <> 'refunded'")
```

## MySQL is not judged

MySQL compares with case-insensitive collations by default.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "mysql.sql"
```

`mysql.sql`:

```sql
CREATE TABLE orders (id INT PRIMARY KEY, status ENUM('pending', 'paid') NOT NULL);
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("-- sql\nSELECT id FROM orders WHERE status = 'PAID'")
```

## Removing a value breaks the code that filters on it

`schema.sql`:

```sql
CREATE TABLE orders (id SERIAL PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded')));
```

`against.sql`:

```sql
CREATE TABLE orders (id SERIAL PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid')));
```

```python
from sqlty import SQL

# error: [unmatched-literal] "`orders.status` is never 'refunded', so this WHERE matches no row"
refunded = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE status = 'refunded'")
```

# ungrouped-column

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
ungrouped-column = "error"
stale-type = "ignore"
narrow-type = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, team TEXT);
CREATE TABLE orders (id INTEGER NOT NULL, user_id INTEGER NOT NULL, status TEXT NOT NULL, total INTEGER NOT NULL);
```

## Not grouped

```python
from sqlty import SQL

# error: [ungrouped-column] "column `orders.total` must appear in GROUP BY"
a = SQL[tuple[str, int]]("--sql\nSELECT status, total FROM orders GROUP BY status")
# error: [ungrouped-column] "column `o.user_id` must appear in GROUP BY"
b = SQL[tuple[int, int]]("--sql\nSELECT o.user_id, count(*) FROM orders o")
# error: [ungrouped-column] "column `orders.id` must appear in GROUP BY"
c = SQL[tuple[int, int]]("--sql\nSELECT id + 1, count(*) FROM orders GROUP BY status")
# error: [ungrouped-column] "column `orders.status` must appear in GROUP BY"
d = SQL[tuple[str, int]]("--sql\nSELECT status, sum(total) FROM orders GROUP BY ROLLUP (user_id)")
```

## Grouped

```python
from sqlty import SQL

# error:
a = SQL[tuple[str, int]]("--sql\nSELECT status, sum(total) FROM orders GROUP BY status")
b = SQL[tuple[str, int]]("--sql\nSELECT o.status, sum(total) FROM orders AS o GROUP BY status")
c = SQL[tuple[str, int]]("--sql\nSELECT lower(status), count(*) FROM orders GROUP BY lower(status)")
d = SQL[tuple[str, int]]("--sql\nSELECT lower(status) AS s, count(*) FROM orders GROUP BY s")
e = SQL[tuple[str, int]]("--sql\nSELECT lower(status), count(*) FROM orders GROUP BY 1")
f = SQL[tuple[str | None, int | None]]("--sql\nSELECT status, sum(total) FROM orders GROUP BY ROLLUP (status)")
g = SQL[tuple[int, int]]("--sql\nSELECT user_id, status, count(*) FROM orders GROUP BY (user_id, status)")
```

## Primary keys

Grouping by a table's primary key covers its other columns. `orders` has no
primary key, so its `id` covers nothing.

```python
from sqlty import SQL

# error: [ungrouped-column] "column `o.status` must appear in GROUP BY"
a = SQL[tuple[int, str, str, int]]("--sql\nSELECT u.id, u.name, o.status, count(*) FROM users u JOIN orders o ON o.user_id = u.id GROUP BY u.id")
```

## Not judged

Aggregates, window functions and subqueries hold their own columns, and a
column of a derived table, a CTE or an unknown table is not judged.

```python
from sqlty import SQL

# error:
a = SQL[tuple[int, int]]("--sql\nSELECT count(*), max(total) FROM orders")
b = SQL[tuple[int, int]]("--sql\nSELECT user_id, rank() OVER (ORDER BY user_id) FROM orders")
c = SQL[tuple[str, int]]("--sql\nSELECT status, (SELECT count(*) FROM users WHERE users.id = 1) FROM orders GROUP BY status")
d = SQL[tuple[str, int]]("--sql\nSELECT s.status, count(*) FROM (SELECT status, total FROM orders) s GROUP BY total")
e = SQL[tuple[int]]("--sql\nWITH t AS (SELECT status FROM orders) SELECT count(*) FROM t")
f = SQL[tuple[str, int]]("--sql\nSELECT status, count(*) FROM orders GROUP BY status HAVING max(total) > 1")
```

## MySQL

`ONLY_FULL_GROUP_BY` follows more dependencies than primary keys, so MySQL is
not judged.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[str, int]]("-- sql\nSELECT status, total FROM orders GROUP BY status")
```

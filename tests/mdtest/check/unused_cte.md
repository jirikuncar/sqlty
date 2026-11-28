# Lint: unused CTEs

`unused-cte` reports a CTE that neither a later CTE nor the statement reads, on its
name.

```toml
[tool.sqlty.rules]
stale-rewrite = "ignore"
```

## PostgreSQL

```python
from sqlty import SQL

# error[22:28]: [unused-cte] "CTE `unused` is never read"
a = SQL("--sql\nWITH unused AS (SELECT 1) SELECT 2")
# error: [unused-cte] "CTE `b` is never read"
b = SQL("--sql\nWITH a AS (SELECT 1 AS x), b AS (SELECT x FROM a) SELECT 2")
```

Reads by a later CTE, a subquery, `TABLE` and a recursive CTE read by the statement count:

```python
from sqlty import SQL

# error:
a = SQL("--sql\nWITH a AS (SELECT 1 AS x), b AS (SELECT x FROM a) SELECT x FROM b")
b = SQL("--sql\nWITH a AS (SELECT 1 AS x) SELECT 2 WHERE 1 IN (SELECT x FROM a)")
c = SQL("--sql\nWITH a AS (SELECT 1 AS x) TABLE a")
d = SQL("--sql\nWITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 3) SELECT n FROM r")
e = SQL("--sql\nWITH \"A\" AS (SELECT 1 AS x) SELECT x FROM \"A\"")
```

A recursive CTE that only reads itself never runs:

```python
from sqlty import SQL

# error: [unused-cte] "CTE `r` is never read"
a = SQL("--sql\nWITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r) SELECT 1")
```

A data-modifying CTE runs unread:

```python
from sqlty import SQL

# error:
a = SQL("--sql\nWITH gone AS (DELETE FROM users RETURNING id) SELECT 1")
```

A table function of the same name does not read it:

```python
from sqlty import SQL

# error: [unused-cte] "CTE `generate_series` is never read"
a = SQL("--sql\nWITH generate_series AS (SELECT 1 AS n) SELECT * FROM generate_series(1, 3)")
```

A name resolves to the innermost CTE of that name:

```python
from sqlty import SQL

# error: [unused-cte] "CTE `c` is never read"
a = SQL("--sql\nWITH c AS (SELECT 1 AS x) SELECT (WITH c AS (SELECT 2 AS x) SELECT x FROM c)")
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [unused-cte] "CTE `unused` is never read"
a = SQL("-- sql\nWITH unused AS (SELECT 1) SELECT 2")
```

## SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [unused-cte] "CTE `unused` is never read"
a = SQL("--sql\nWITH unused AS (SELECT 1) SELECT 2")
```

Any CTE of a SQLite `WITH` may read any other, `RECURSIVE` or not:

```python
from sqlty import SQL

# error:
a = SQL("--sql\nWITH a AS (SELECT * FROM b), b AS (SELECT 1 AS x) SELECT * FROM a")
b = SQL("--sql\nWITH r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 3) SELECT n FROM r")
```

## DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
```

```python
from sqlty import SQL

# error: [unused-cte] "CTE `unused` is never read"
a = SQL("--sql\nWITH unused AS (SELECT 1) SELECT 2")
# error:
b = SQL("--sql\nWITH a AS (SELECT 1 AS x) FROM a")
```

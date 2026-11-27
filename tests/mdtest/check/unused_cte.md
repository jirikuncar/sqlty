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

Reads by a later CTE, a subquery, `TABLE` and a recursive CTE's own reference count:

```python
from sqlty import SQL

# error:
a = SQL("--sql\nWITH a AS (SELECT 1 AS x), b AS (SELECT x FROM a) SELECT x FROM b")
b = SQL("--sql\nWITH a AS (SELECT 1 AS x) SELECT 2 WHERE 1 IN (SELECT x FROM a)")
c = SQL("--sql\nWITH a AS (SELECT 1 AS x) TABLE a")
d = SQL("--sql\nWITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 3) SELECT 1")
e = SQL("--sql\nWITH \"A\" AS (SELECT 1 AS x) SELECT x FROM \"A\"")
```

A data-modifying CTE runs unread:

```python
from sqlty import SQL

# error:
a = SQL("--sql\nWITH gone AS (DELETE FROM users RETURNING id) SELECT 1")
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

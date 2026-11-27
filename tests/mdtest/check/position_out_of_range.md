# Lint: ORDER BY and GROUP BY positions

`position-out-of-range` reports an integer `ORDER BY` or `GROUP BY` key that names no
column of the select list, on the integer.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
select-star = "ignore"
stale-rewrite = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
CREATE TABLE orgs (id INTEGER NOT NULL);
```

## PostgreSQL

```python
from sqlty import SQL

# error[70:71]: [position-out-of-range] "ORDER BY position 5 is not in the select list (2 columns)"
a = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users ORDER BY 5")
# error: [position-out-of-range]
b = SQL[tuple[int]]("--sql\nSELECT id FROM users ORDER BY 0")
# error: [position-out-of-range] "ORDER BY position -1 is not in the select list (1 column)"
c = SQL[tuple[int]]("--sql\nSELECT id FROM users ORDER BY -1")
# error: [position-out-of-range] "ORDER BY position 2 is not in the select list (1 column)"
d = SQL[tuple[int]]("--sql\nSELECT id FROM users UNION SELECT id FROM orgs ORDER BY 2")
# error: [position-out-of-range] "ORDER BY position 3 is not in the select list (2 columns)"
e = SQL[tuple[int, int]]("--sql\nSELECT * FROM orgs, orgs o ORDER BY 3")
```

A `GROUP BY` position out of range is reported once, without `ungrouped-column`:

```python
from sqlty import SQL

# error: [position-out-of-range] "GROUP BY position 4 is not in the select list (2 columns)"
a = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users GROUP BY 4")
```

Positions in parentheses and in grouping sets count too:

```python
from sqlty import SQL

# error: [position-out-of-range] "ORDER BY position 5 is not in the select list (1 column)"
a = SQL[tuple[int]]("--sql\nSELECT id FROM users ORDER BY (5)")
# error: [position-out-of-range] "GROUP BY position 5 is not in the select list (2 columns)"
b = SQL[tuple[int | None, str]]("--sql\nSELECT id, name FROM users GROUP BY ROLLUP(id, 5)")
```

An integer wider than 32 bits is a constant, and `+5` an expression:

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT id FROM users ORDER BY 3000000000")
b = SQL[tuple[int]]("--sql\nSELECT id FROM users ORDER BY +5")
```

Positions in range, other expressions, and a select list of unknown width are not
reported.

```python
from sqlty import SQL

a = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users GROUP BY 1, 2 ORDER BY 2, id + 1")
# error: [unknown-table]
b = SQL("--sql\nSELECT * FROM missing ORDER BY 9")
```

## MySQL

A negative number is an expression in MySQL.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [position-out-of-range]
a = SQL[tuple[int, str]]("-- sql\nSELECT id, name FROM users ORDER BY 3")
# error: [position-out-of-range]
b = SQL[tuple[int, str]]("-- sql\nSELECT id, name FROM users GROUP BY 0")
c = SQL[tuple[int]]("-- sql\nSELECT id FROM users ORDER BY -1")
```

## SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [position-out-of-range]
a = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users ORDER BY 3")
# error: [position-out-of-range]
b = SQL[tuple[int]]("--sql\nSELECT id FROM users GROUP BY -1")
# error: [position-out-of-range]
c = SQL[tuple[int]]("--sql\nSELECT id FROM users ORDER BY +5")
```

## DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
```

```python
from sqlty import SQL

# error: [position-out-of-range]
a = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users ORDER BY 3")
# error: [position-out-of-range]
b = SQL[tuple[int]]("--sql\nSELECT id FROM users GROUP BY 2")
# error: [position-out-of-range]
c = SQL[tuple[int]]("--sql\nSELECT id FROM users ORDER BY 3000000000")
```

## In a condition subquery

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from sqlty import SQL

# error: [position-out-of-range]
a = SQL[tuple[int]]("--sql\nSELECT id FROM users WHERE id IN (SELECT id FROM users ORDER BY 2)")
```

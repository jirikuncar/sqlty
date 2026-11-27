# Lint: set operation column counts

`set-column-count` reports a `UNION`, `INTERSECT` or `EXCEPT` branch that returns
another number of columns than the first one, on that branch.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
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

# error[50:69]: [set-column-count] "each UNION branch must return 2 columns, this one returns 1"
a = SQL("--sql\nSELECT id, name FROM users UNION SELECT id FROM orgs")
# error: [set-column-count] "each INTERSECT branch must return 1 column, this one returns 2"
b = SQL("--sql\nSELECT id FROM orgs INTERSECT SELECT id, name FROM users")
# error: [set-column-count] "each EXCEPT branch must return 1 column, this one returns 2"
c = SQL("--sql\nSELECT id FROM orgs UNION SELECT id FROM orgs EXCEPT SELECT id, 1 FROM orgs")
# error: [set-column-count] "each UNION branch must return 1 column, this one returns 2"
d = SQL("--sql\nSELECT id FROM orgs UNION (SELECT id, name FROM users)")
# error: [set-column-count] "each UNION branch must return 2 columns, this one returns 1"
e = SQL("--sql\nSELECT id, name FROM users UNION ALL VALUES (1)")
# error: [set-column-count] "each UNION branch must return 2 columns, this one returns 1"
f = SQL("--sql\nSELECT * FROM users UNION SELECT id FROM orgs")
# error: [set-column-count] "each UNION branch must return 1 column, this one returns 2"
g = SQL("--sql\nSELECT id FROM users WHERE id IN (SELECT id FROM orgs UNION SELECT id, name FROM users)")
# error: [set-column-count] "each UNION branch must return 1 column, this one returns 2"
h = SQL("--sql\nSELECT id FROM orgs UNION (SELECT id, name FROM users EXCEPT SELECT id, name FROM users)")
```

The first branch of known width sets the count:

```python
from sqlty import SQL

# error: [unknown-table]
# error: [set-column-count] "each UNION branch must return 1 column, this one returns 2"
a = SQL("--sql\nSELECT * FROM missing UNION SELECT id FROM orgs UNION SELECT id, name FROM users")
```

Branches of the same width, and a branch whose width is unknown, are not
reported.

```python
from sqlty import SQL

# error: [unknown-table] "table `missing` is not defined in the schema"
a = SQL("--sql\nSELECT id, name FROM users UNION SELECT * FROM missing")
b = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users UNION SELECT id, 'x' FROM orgs")
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [set-column-count] "each UNION branch must return 2 columns, this one returns 1"
a = SQL("-- sql\nSELECT id, name FROM users UNION SELECT id FROM orgs")
```

## SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [set-column-count] "each UNION branch must return 2 columns, this one returns 1"
a = SQL("--sql\nSELECT id, name FROM users UNION SELECT id FROM orgs")
```

## DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
```

```python
from sqlty import SQL

# error: [set-column-count] "each UNION branch must return 2 columns, this one returns 1"
a = SQL("--sql\nSELECT id, name FROM users UNION SELECT id FROM orgs")
```

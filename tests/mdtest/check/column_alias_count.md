# Lint: column alias lists

`column-alias-count` reports a column alias list that names more columns than the
relation it renames has, on the list.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
select-star = "ignore"
stale-rewrite = "ignore"
unused-cte = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
CREATE FUNCTION pair() RETURNS TABLE (a INTEGER, b INTEGER) LANGUAGE sql AS $$ SELECT 1, 2 $$;
CREATE VIEW ids AS SELECT id FROM users;
CREATE TYPE point2 AS (x INTEGER, y INTEGER);
CREATE FUNCTION origin() RETURNS point2 LANGUAGE sql AS $$ SELECT (0, 0)::point2 $$;
```

## PostgreSQL

```python
from sqlty import SQL

# error[49:56]: [column-alias-count] "`x` has 1 column but 3 aliases"
a = SQL("--sql\nSELECT x.a FROM (SELECT 1) AS x(a, b, c)")
# error: [column-alias-count] "`t` has 1 column but 2 aliases"
b = SQL("--sql\nWITH t(a, b) AS (SELECT 1) SELECT a FROM t")
# error: [column-alias-count] "`u` has 2 columns but 3 aliases"
c = SQL("--sql\nSELECT 1 FROM users AS u(a, b, c)")
# error: [column-alias-count] "`g` has 1 column but 2 aliases"
d = SQL("--sql\nSELECT g.a FROM generate_series(1, 3) AS g(a, b)")
# error: [column-alias-count] "`p` has 2 columns but 3 aliases"
e = SQL("--sql\nSELECT p.x FROM pair() AS p(x, y, z)")
```

Views and CTEs read under an alias list count too:

```python
from sqlty import SQL

# error: [column-alias-count] "`x` has 1 column but 2 aliases"
a = SQL("--sql\nSELECT 1 FROM ids AS x(a, b)")
# error: [column-alias-count] "`x` has 1 column but 2 aliases"
b = SQL("--sql\nWITH t AS (SELECT 1) SELECT 1 FROM t AS x(a, b)")
```

A function returning one composite value expands into its fields:

```python
from sqlty import SQL

# error:
a = SQL("--sql\nSELECT 1 FROM origin() AS o(x, y)")
b = SQL("--sql\nSELECT 1 FROM pg_get_keywords() AS k(word, catcode, barelabel, catdesc, baredesc)")
```

A shorter list keeps the names of the remaining columns, and a relation of unknown
width is not judged:

```python
from sqlty import SQL

# error:
a = SQL("--sql\nSELECT x.a FROM (SELECT 1, 2) AS x(a)")
b = SQL("--sql\nSELECT g.a, g.n FROM generate_series(1, 3) WITH ORDINALITY AS g(a, n)")
c = SQL("--sql\nSELECT 1 FROM users AS u(a, b)")
```

## MySQL

Both counts must match for a CTE or derived table.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [column-alias-count] "`t` has 1 column but 2 aliases"
a = SQL("-- sql\nWITH t(a, b) AS (SELECT 1) SELECT a FROM t")
# error: [column-alias-count] "`x` has 2 columns but 1 alias"
b = SQL("-- sql\nSELECT x.a FROM (SELECT 1, 2) AS x(a)")
```

## SQLite

Both counts must match for a CTE.

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [column-alias-count] "`t` has 2 columns but 1 alias"
a = SQL("--sql\nWITH t(a) AS (SELECT 1, 2) SELECT a FROM t")
```

## DuckDB

A DuckDB CTE or table function takes a longer list.

```toml
[tool.sqlty]
dialect = "duckdb"
```

```python
from sqlty import SQL

# error: [column-alias-count] "`x` has 1 column but 3 aliases"
a = SQL("--sql\nSELECT x.a FROM (SELECT 1) AS x(a, b, c)")
# error: [column-alias-count] "`u` has 2 columns but 3 aliases"
b = SQL("--sql\nSELECT 1 FROM users AS u(a, b, c)")
```

```python
from sqlty import SQL

# error:
a = SQL("--sql\nWITH t(a, b) AS (SELECT 1) SELECT a FROM t")
```

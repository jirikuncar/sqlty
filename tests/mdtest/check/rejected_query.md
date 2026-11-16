# rejected-query

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "postgres"

[tool.sqlty.rules]
stale-rewrite = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE t (i INT, d DATE, ts TIMESTAMPTZ, b BOOLEAN NOT NULL, tx TEXT, n NUMERIC, iv INTERVAL, tm TIME);
```

## Results of different type categories

```python
from sqlty import SQL

# error: [rejected-query] "CASE types TEXT and INT cannot be matched"
a = SQL("--sql\nSELECT CASE WHEN b THEN tx ELSE i END FROM t")
# error: [rejected-query] "CASE types BOOLEAN and INT cannot be matched"
b = SQL("--sql\nSELECT CASE WHEN b THEN true ELSE 1 END FROM t")
# error: [rejected-query] "CASE types INTERVAL and TIME cannot be matched"
c = SQL("--sql\nSELECT CASE WHEN b THEN iv ELSE tm END FROM t")
# error: [rejected-query] "argument types INT and DATE cannot be matched"
d = SQL("--sql\nSELECT coalesce(i, d) FROM t")
```

## Results of one category

Numbers, dates and timestamps promote; a string literal, `NULL` and a parameter
take the type of the other results.

```python
from sqlty import SQL

# error:
a = SQL("--sql\nSELECT greatest(i, n), coalesce(d, ts), coalesce(d, '2020-01-01') FROM t")
b = SQL("--sql\nSELECT coalesce(i, $1), CASE WHEN b THEN NULL ELSE d END FROM t")
```

## JSON and UUID match no other category

`json`, `jsonb` and `uuid` are user-defined types (category `U`) to PostgreSQL,
which never match a string or a number: `COALESCE types jsonb and text cannot
be matched`. `json` and `jsonb` cast to each other, and a string literal or a
parameter takes their type.

`schema.sql`:

```sql
CREATE TABLE d (j JSONB, js JSON, u UUID, tx TEXT, i INT, b BOOLEAN NOT NULL);
```

```python
from sqlty import SQL

# error: [rejected-query] "argument types JSONB and TEXT cannot be matched"
a = SQL("--sql\nSELECT coalesce(j, tx) FROM d")
# error: [rejected-query] "argument types UUID and TEXT cannot be matched"
b = SQL("--sql\nSELECT coalesce(u, tx) FROM d")
# error: [rejected-query] "CASE types JSONB and TEXT cannot be matched"
c = SQL("--sql\nSELECT CASE WHEN b THEN j ELSE tx END FROM d")
# error: [rejected-query] "argument types UUID and JSONB cannot be matched"
e = SQL("--sql\nSELECT coalesce(u, j) FROM d")
# error: [rejected-query] "argument types JSON and INT cannot be matched"
f = SQL("--sql\nSELECT coalesce(js, i) FROM d")
```

```python
from sqlty import SQL

# error:
a = SQL("--sql\nSELECT coalesce(j, js), coalesce(u, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'), coalesce(j, '{}'), coalesce(u, $1) FROM d")
```

A rejected expression has no type:

```sql
SELECT coalesce(j, tx) AS a, CASE WHEN b THEN u ELSE tx END AS c FROM d
-- columns: a: None (None)
-- columns: c: None (None)
```

## Other dialects

Only PostgreSQL is judged: MySQL and SQLite convert the results.

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error:
a = SQL("--sql\nSELECT CASE WHEN b THEN tx ELSE i END FROM t")
```

## SELECT * with no FROM

```python
from sqlty import SQL

# error: [rejected-query] "`SELECT *` with no FROM selects no columns"
a = SQL("--sql\nSELECT *")
```

### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [rejected-query] "`SELECT *` with no FROM selects no columns"
a = SQL("--sql\nSELECT *")
```

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [rejected-query] "`SELECT *` with no FROM selects no columns"
a = SQL("-- sql\nSELECT *")
```

### DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
```

```python
from sqlty import SQL

# error: [rejected-query] "`SELECT *` with no FROM selects no columns"
a = SQL("--sql\nSELECT *")
```

### ClickHouse reads system.one

```toml
[tool.sqlty]
dialect = "clickhouse"
```

```python
from sqlty import SQL

# error:
a = SQL("--sql\nSELECT *")
```

## SQLite hex literals

A hex literal is a 64-bit integer, in two's complement.

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [rejected-query] "hex literal too big: 0x1FFFFFFFFFFFFFFFF"
a = SQL("--sql\nSELECT 0x1FFFFFFFFFFFFFFFF")
# error:
b = SQL[tuple[int, bytes]]("--sql\nSELECT 0xFFFFFFFFFFFFFFFF, X'AB'")
```

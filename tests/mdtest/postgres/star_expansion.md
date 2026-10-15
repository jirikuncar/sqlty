# Star Expansion

## Single table — all columns expanded

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM users
-- expanded: SELECT id::INT, name::TEXT, email::VARCHAR(255) FROM users
```

## Qualified star — table prefix

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT users.* FROM users
-- expanded: SELECT users.id::INT, users.name::TEXT FROM users
```

## No schema — returns None

```sql
SELECT * FROM users
-- expanded: None
```

## No star — returns None

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
-- expanded: None
```

## Table alias with qualified star

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT u.* FROM users u
-- expanded: SELECT u.id::INT, u.name::TEXT FROM users u
```

## Multi-table star is not expanded

A bare `*` expands to unqualified columns, so a query reading from more than
one table (`JOIN` or `FROM a, b`) is left alone. A qualified `t.*` keeps its
qualifier and is expanded.

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE orders (
    order_id INTEGER PRIMARY KEY,
    amount NUMERIC NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM users, orders
-- expanded: None
```

## Unknown table — returns None

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM unknown_table
-- expanded: None
```

## Other projections are kept verbatim

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT *, id::INTEGER + 1 AS next_id FROM users
-- expanded: SELECT id::INT, name::TEXT, id::INTEGER + 1 AS next_id FROM users
```

## `expand_star`

### Expands with schema

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```sql
SELECT * FROM users
-- expanded: SELECT id::INT, name::TEXT FROM users
```

### Non star returns none

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```sql
SELECT id FROM users
-- expanded: None
```

### Returns str or none

```sql
SELECT * FROM t
-- expanded: None
```

## Regression cases

### `CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CRE...`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
SELECT * FROM users
-- expanded: SELECT id::INT, name::TEXT, score::DECIMAL(10, 2) FROM users
```

### `CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CRE...` (2)

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
SELECT o.*, 1 AS one FROM orders o
-- expanded: SELECT o.oid::INT, o.user_id::INT, o.placed::TIMESTAMPTZ, 1 AS one FROM orders o
```

### `CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CRE...` (3)

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
SELECT * FROM users, audit_log
-- expanded: None
```

## Quoted column names keep their quotes

`schema.sql`:
```sql
CREATE TABLE t (id INTEGER, "My Col" TEXT);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT * FROM t
-- expanded: SELECT id::INT, "My Col"::TEXT FROM t
-- columns: id: int | None (INT)
-- columns: My Col: str | None (TEXT)
```

```python
q = SQL[tuple[int, str]]('--sql\nSELECT * FROM t')
# fixed: q = SQL[tuple[int | None, str | None]]('--sql\nSELECT id::INT, "My Col"::TEXT FROM t')
```

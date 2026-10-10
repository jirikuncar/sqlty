# Inference Contract

## `infer_type`

### Single integer

```sql
SELECT id::INTEGER FROM t
-- inferred: tuple[int]
```

### Multiple columns

```sql
SELECT id::INTEGER, name::TEXT FROM t
-- inferred: tuple[int, str]
```

### Float

```sql
SELECT price::FLOAT FROM t
-- inferred: tuple[float]
```

### Boolean

```sql
SELECT active::BOOLEAN FROM t
-- inferred: tuple[bool]
```

### Datetime

```sql
SELECT ts::TIMESTAMP FROM t
-- inferred: tuple[datetime.datetime]
```

### Uuid

```sql
SELECT uid::UUID FROM t
-- inferred: tuple[uuid.UUID]
```

### Returns none for garbage

```sql
not valid sql at all {{{}}
-- inferred: None
```

### Returns none for empty

```sql

-- inferred: None
```

### Dialect postgres

```sql
SELECT 1::INTEGER
-- inferred: tuple[int]
```

### Dialect mysql

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(1 AS SIGNED)
-- inferred: tuple[int]
```

### Dialect sqlite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```sql
SELECT CAST(1 AS INTEGER)
-- inferred: tuple[int]
```

### Schema aware

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```sql
SELECT id, name FROM users
-- inferred: tuple[int, str]
```

### Schema none

```sql
SELECT id FROM users
-- inferred: tuple[str]
```

## Former backend divergences

### `DELETE FROM t RETURNING id::INTEGER`

```sql
DELETE FROM t RETURNING id::INTEGER
-- inferred: tuple[int]
```

### `WITH x AS (SELECT 1) DELETE FROM t RETURNING id::INTEGER`

```sql
WITH x AS (SELECT 1) DELETE FROM t RETURNING id::INTEGER
-- inferred: tuple[int]
```

### `WITH x AS (SELECT 1) UPDATE t SET a = 1 RETURNING id::INTEGER`

```sql
WITH x AS (SELECT 1) UPDATE t SET a = 1 RETURNING id::INTEGER
-- inferred: tuple[int]
```

### `WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x RETURNING id::IN...`

```sql
WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x RETURNING id::INTEGER
-- inferred: tuple[int]
```

### `WITH n AS (INSERT INTO t VALUES (1) RETURNING id) SELECT id::INTEGE...`

```sql
WITH n AS (INSERT INTO t VALUES (1) RETURNING id) SELECT id::INTEGER FROM n
-- inferred: tuple[int]
```

### `INSERT INTO t SELECT a::INTEGER FROM u`

```sql
INSERT INTO t SELECT a::INTEGER FROM u
-- inferred: None
```

### `UPDATE t SET a = (SELECT b::INTEGER FROM u)`

```sql
UPDATE t SET a = (SELECT b::INTEGER FROM u)
-- inferred: None
```

### `CREATE TABLE x AS SELECT a::INTEGER FROM t`

```sql
CREATE TABLE x AS SELECT a::INTEGER FROM t
-- inferred: None
```

### `SELECT a::INTEGER FROM t UNION SELECT b FROM u UNION SELECT c FROM v`

```sql
SELECT a::INTEGER FROM t UNION SELECT b FROM u UNION SELECT c FROM v
-- inferred: tuple[int]
```

### `SELECT 1, 1.5, 'a', TRUE, NULL`

```sql
SELECT 1, 1.5, 'a', TRUE, NULL
-- inferred: tuple[int, float, str, bool, str]
```

### `SELECT COUNT(*) AS n, SUM(x) AS s FROM t`

```sql
SELECT COUNT(*) AS n, SUM(x) AS s FROM t
-- inferred: tuple[int, float]
```

### `SELECT AVG(x::INTEGER), MIN(x::INTEGER), MAX(d::DATE) FROM t`

```sql
SELECT AVG(x::INTEGER), MIN(x::INTEGER), MAX(d::DATE) FROM t
-- inferred: tuple[float, int, datetime.date]
```

### `SELECT COALESCE(x::INTEGER, 0), (x::INTEGER) FROM t`

```sql
SELECT COALESCE(x::INTEGER, 0), (x::INTEGER) FROM t
-- inferred: tuple[int, int]
```

### `SELECT CASE WHEN a THEN 1::INTEGER ELSE 2 END FROM t`

```sql
SELECT CASE WHEN a THEN 1::INTEGER ELSE 2 END FROM t
-- inferred: tuple[int]
```

### `SELECT EXISTS (SELECT 1), x NOT IN (SELECT 1) FROM t`

```sql
SELECT EXISTS (SELECT 1), x NOT IN (SELECT 1) FROM t
-- inferred: tuple[bool, bool]
```

### `SELECT (SELECT y::INTEGER FROM u) FROM t`

```sql
SELECT (SELECT y::INTEGER FROM u) FROM t
-- inferred: tuple[int]
```

### `SELECT ROW_NUMBER() OVER (), json_build_object('a', 1) FROM t`

```sql
SELECT ROW_NUMBER() OVER (), json_build_object('a', 1) FROM t
-- inferred: tuple[int, dict]
```

### `SELECT ts::DOUBLE PRECISION, tags::INT[] FROM t`

```sql
SELECT ts::DOUBLE PRECISION, tags::INT[] FROM t
-- inferred: tuple[float, list]
```

### Schema: `SELECT * FROM users`

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
-- inferred: tuple[int, str, float]
```

### Schema: `SELECT o.*, u.name FROM users u JOIN orders o ON TRUE`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
SELECT o.*, u.name FROM users u JOIN orders o ON TRUE
-- inferred: tuple[int, int, datetime.datetime, str]
```

### Schema: `SELECT u.id, placed FROM users u JOIN orders o ON TRUE`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
SELECT u.id, placed FROM users u JOIN orders o ON TRUE
-- inferred: tuple[int, datetime.datetime]
```

### Schema: `SELECT * FROM audit_log`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
SELECT * FROM audit_log
-- inferred: tuple[str]
```

### Schema: `SELECT MAX(placed), AVG(oid) FROM orders`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
SELECT MAX(placed), AVG(oid) FROM orders
-- inferred: tuple[datetime.datetime, float]
```

### Schema: `DELETE FROM users RETURNING *`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
DELETE FROM users RETURNING *
-- inferred: tuple[int, str, float]
```

### Schema: `DELETE FROM users RETURNING id, name`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10,2));CREATE TABLE orders (oid INT4, user_id INT, placed TIMESTAMP WITH TIME ZONE);
```

```sql
DELETE FROM users RETURNING id, name
-- inferred: tuple[int, str]
```

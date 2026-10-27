# Schema columns through CTEs and derived tables

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## CTE

```sql
WITH recent AS (SELECT id, name FROM users)
SELECT id, name FROM recent
-- inferred: tuple[int, str]
```

## Later CTEs see earlier ones

```sql
WITH a AS (SELECT id, email FROM users), b AS (SELECT email FROM a)
SELECT email FROM b
-- inferred: tuple[str | None]
```

## CTE column alias list

```sql
WITH r(uid, label) AS (SELECT id, name FROM users)
SELECT label, uid FROM r
-- inferred: tuple[str, int]
```

## Star over a CTE

```sql
WITH r AS (SELECT id, created_at FROM users)
SELECT * FROM r
-- inferred: tuple[int, datetime.datetime | None]
```

## Recursive CTE uses the non-recursive term

```sql
WITH RECURSIVE t AS (
    SELECT id, name FROM users
    UNION ALL
    SELECT id, name FROM t
)
SELECT id, name FROM t
-- inferred: tuple[int, str]
```

## Writable CTE exposes RETURNING columns

```sql
WITH new_user AS (
    INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')
    RETURNING id, name, email
)
SELECT id, name, email FROM new_user
-- inferred: tuple[int, str, str]
```

## Derived table

```sql
SELECT t.id, t.name FROM (SELECT id, name FROM users) AS t
-- inferred: tuple[int, str]
```

## Derived table with column aliases

```sql
SELECT x, y FROM (SELECT id, name FROM users) AS t(x, y)
-- inferred: tuple[int, str]
```

## Star over a derived table

```sql
SELECT t.* FROM (SELECT name, created_at FROM users) AS t
-- inferred: tuple[str, datetime.datetime | None]
```

## LATERAL join

```sql
SELECT u.id, o.status
FROM users u
JOIN LATERAL (SELECT status FROM orders WHERE user_id = u.id LIMIT 1) o ON true
-- inferred: tuple[int, str | None]
```

## A CTE shadows a schema table of the same name

```sql
WITH users AS (SELECT 1 AS other)
SELECT other FROM users
-- inferred: tuple[int]
```

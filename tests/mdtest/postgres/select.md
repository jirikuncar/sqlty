# PostgreSQL SELECT Queries

```toml
[tool.sqlty]
dialect = "postgres"
```

## Simple SELECT with type casts

```sql
SELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users
-- inferred: tuple[int, str, str]
```

## SELECT without casts defaults to str

```sql
SELECT id, name, email FROM users
-- inferred: tuple[str, str, str]
```

## SELECT with WHERE clause

```sql
SELECT id::INTEGER, name::TEXT FROM users WHERE active = true
-- inferred: tuple[int, str]
```

## SELECT with INNER JOIN

```sql
SELECT u.id::INTEGER, u.name::TEXT, o.total::FLOAT
FROM users u
INNER JOIN orders o ON u.id = o.user_id
-- inferred: tuple[int, str, float]
```

## SELECT with LEFT JOIN and COALESCE

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    COALESCE(COUNT(o.id), 0)::INTEGER
FROM users u
LEFT JOIN orders o ON u.id = o.user_id
GROUP BY u.id, u.name
-- inferred: tuple[int, str, int]
```

## Subquery in SELECT

```sql
SELECT
    id::INTEGER,
    name::TEXT,
    (SELECT COUNT(*) FROM orders WHERE user_id = users.id)::INTEGER
FROM users
-- inferred: tuple[int, str, int]
```

## SELECT DISTINCT

```sql
SELECT DISTINCT category::TEXT, COUNT(*)::INTEGER
FROM products
GROUP BY category
-- inferred: tuple[str, int]
```

## SELECT with LIMIT and OFFSET

```sql
SELECT id::INTEGER, name::TEXT FROM users LIMIT 10 OFFSET 20
-- inferred: tuple[int, str]
```

## SELECT with ORDER BY

```sql
SELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP
FROM users
ORDER BY created_at DESC
-- inferred: tuple[int, str, datetime.datetime]
```

## SELECT with CASE expression

```sql
SELECT
    id::INTEGER,
    CASE WHEN active THEN 'active'::TEXT ELSE 'inactive'::TEXT END
FROM users
-- inferred: tuple[int, str]
```

## Named parameters (PostgreSQL :param style)

```sql
SELECT id::INTEGER, name::TEXT FROM users WHERE id = :user_id AND active = :active
-- inferred: tuple[int, str]
```

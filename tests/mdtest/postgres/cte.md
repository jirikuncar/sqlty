# PostgreSQL Common Table Expressions (CTEs)

```toml
[tool.sqlty]
dialect = "postgres"
```

## Simple CTE

```sql
WITH active_users AS (
    SELECT id, name FROM users WHERE active = true
)
SELECT id::INTEGER, name::TEXT FROM active_users
-- inferred: tuple[int, str]
```

## Multiple CTEs

```sql
WITH
    active_users AS (
        SELECT id, name FROM users WHERE active = true
    ),
    user_orders AS (
        SELECT user_id, COUNT(*) as order_count
        FROM orders
        GROUP BY user_id
    )
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    COALESCE(o.order_count, 0)::INTEGER
FROM active_users u
LEFT JOIN user_orders o ON u.id = o.user_id
-- inferred: tuple[int, str, int]
```

## WITH RECURSIVE — tree traversal

```sql
WITH RECURSIVE category_tree AS (
    SELECT id, name, parent_id, 0 as depth
    FROM categories
    WHERE parent_id IS NULL

    UNION ALL

    SELECT c.id, c.name, c.parent_id, ct.depth + 1
    FROM categories c
    JOIN category_tree ct ON c.parent_id = ct.id
)
SELECT id::INTEGER, name::TEXT, depth::INTEGER
FROM category_tree
ORDER BY depth, name
-- inferred: tuple[int, str, int]
```

## WITH RECURSIVE — generate series equivalent

```sql
WITH RECURSIVE counter AS (
    SELECT 1::INTEGER as n
    UNION ALL
    SELECT (n + 1)::INTEGER FROM counter WHERE n < 100
)
SELECT n::INTEGER FROM counter
-- inferred: tuple[int]
```

## CTE with INSERT RETURNING (writable CTE)

Known limitation: the analyzer finds the CTE's inner RETURNING clause (untyped)
before the outer SELECT (typed), so types come from the RETURNING not the SELECT.

```sql
WITH new_user AS (
    INSERT INTO users (name, email)
    VALUES ('Alice', 'alice@example.com')
    RETURNING id, name, email
)
SELECT id::INTEGER, name::TEXT, email::TEXT FROM new_user
-- inferred: tuple[int, str, str]
```

## CTE with DELETE RETURNING (writable CTE)

Same limitation as above — inner RETURNING wins over outer SELECT casts.

```sql
WITH deleted_users AS (
    DELETE FROM users
    WHERE last_login < NOW() - INTERVAL '2 years'
    RETURNING id, name, email
)
SELECT id::INTEGER, name::TEXT, email::TEXT FROM deleted_users
-- inferred: tuple[int, str, str]
```

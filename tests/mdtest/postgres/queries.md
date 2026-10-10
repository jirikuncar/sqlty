# Query Patterns

## SELECT with WHERE clause

```sql
SELECT id::INTEGER, email::TEXT FROM users WHERE active::BOOLEAN = true
-- inferred: tuple[int, str]
```

## SELECT with JOINs

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    p.title::TEXT
FROM users u
JOIN posts p ON u.id = p.user_id
-- inferred: tuple[int, str, str]
```

## SELECT with multiple JOINs

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    p.title::TEXT,
    c.content::TEXT
FROM users u
JOIN posts p ON u.id = p.user_id
JOIN comments c ON p.id = c.post_id
-- inferred: tuple[int, str, str, str]
```

## COUNT aggregation

```sql
SELECT COUNT(*)::INTEGER FROM users
-- inferred: tuple[int]
```

## SUM aggregation

```sql
SELECT SUM(price)::FLOAT FROM products
-- inferred: tuple[float]
```

## AVG aggregation

```sql
SELECT AVG(score)::FLOAT FROM reviews
-- inferred: tuple[float]
```

## MIN and MAX aggregations

```sql
SELECT
    MIN(price)::FLOAT,
    MAX(price)::FLOAT
FROM products
-- inferred: tuple[float, float]
```

## GROUP BY with aggregations

```sql
SELECT
    category::TEXT,
    COUNT(*)::INTEGER,
    AVG(price)::FLOAT
FROM products
GROUP BY category
-- inferred: tuple[str, int, float]
```

## HAVING clause with aggregations

```sql
SELECT
    user_id::INTEGER,
    COUNT(*)::INTEGER
FROM posts
GROUP BY user_id
HAVING COUNT(*) > 10
-- inferred: tuple[int, int]
```

## JSON field extraction

```sql
SELECT
    id::INTEGER,
    (data->>'name')::TEXT
FROM users
-- inferred: tuple[int, str]
```

## Nested JSON extraction

```sql
SELECT
    id::INTEGER,
    (data->'profile'->>'email')::TEXT
FROM users
-- inferred: tuple[int, str]
```

## Jsonb_agg aggregation

```sql
SELECT
    category::TEXT,
    jsonb_agg(name)::TEXT
FROM products
GROUP BY category
-- inferred: tuple[str, str]
```

## Json_build_object

```sql
SELECT
    json_build_object('id', id, 'name', name)::TEXT
FROM users
-- inferred: tuple[str]
```

## Nested JSON aggregation with explicit cast

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    COALESCE(jsonb_agg(p.title), '[]'::jsonb)::TEXT
FROM users u
LEFT JOIN posts p ON u.id = p.user_id
GROUP BY u.id, u.name
-- inferred: tuple[int, str, str]
```

## Date and time types

```sql
SELECT
    created_date::DATE,
    created_at::TIMESTAMP,
    updated_at::TIMESTAMPTZ,
    scheduled_time::TIME
FROM events
-- inferred: tuple[datetime.date, datetime.datetime, datetime.datetime, datetime.time]
```

## Various numeric types

```sql
SELECT
    tiny::SMALLINT,
    normal::INTEGER,
    big::BIGINT,
    amount::DECIMAL,
    ratio::FLOAT,
    precise::DOUBLE
FROM measurements
-- inferred: tuple[int, int, int, float, float, float]
```

## Binary types

```sql
SELECT
    id::INTEGER,
    document::BYTEA,
    image::BLOB
FROM files
-- inferred: tuple[int, str, str]
```

## BOOLEAN type

```sql
SELECT
    id::INTEGER,
    is_active::BOOLEAN,
    is_verified::BOOL
FROM users
-- inferred: tuple[int, bool, bool]
```

## Subquery in SELECT clause

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    (SELECT COUNT(*)::INTEGER FROM posts WHERE user_id = u.id)
FROM users u
-- inferred: tuple[int, str, int]
```

## Simple CTE (Common Table Expression)

```sql
WITH active_users AS (
    SELECT id, name FROM users WHERE active = true
)
SELECT
    id::INTEGER,
    name::TEXT
FROM active_users
-- inferred: tuple[int, str]
```

## Multiple CTEs

```sql
WITH
    active_users AS (
        SELECT id, name FROM users WHERE active = true
    ),
    user_posts AS (
        SELECT user_id, COUNT(*) as count FROM posts GROUP BY user_id
    )
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    COALESCE(p.count, 0)::INTEGER
FROM active_users u
LEFT JOIN user_posts p ON u.id = p.user_id
-- inferred: tuple[int, str, int]
```

## ROW_NUMBER window function

```sql
SELECT
    id::INTEGER,
    name::TEXT,
    ROW_NUMBER() OVER (ORDER BY created_at)::INTEGER
FROM users
-- inferred: tuple[int, str, int]
```

## RANK and DENSE_RANK

```sql
SELECT
    id::INTEGER,
    score::FLOAT,
    RANK() OVER (ORDER BY score DESC)::INTEGER,
    DENSE_RANK() OVER (ORDER BY score DESC)::INTEGER
FROM scores
-- inferred: tuple[int, float, int, int]
```

## Window function with PARTITION BY

```sql
SELECT
    user_id::INTEGER,
    post_id::INTEGER,
    ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY created_at)::INTEGER
FROM posts
-- inferred: tuple[int, int, int]
```

## UNION

```sql
SELECT id::INTEGER, name::TEXT FROM users
UNION
SELECT id::INTEGER, name::TEXT FROM customers
-- inferred: tuple[int, str]
```

## UNION ALL

```sql
SELECT id::INTEGER, email::TEXT FROM users
UNION ALL
SELECT id::INTEGER, email::TEXT FROM subscribers
-- inferred: tuple[int, str]
```

## INTERSECT

```sql
SELECT id::INTEGER FROM active_users
INTERSECT
SELECT id::INTEGER FROM verified_users
-- inferred: tuple[int]
```

## EXCEPT

```sql
SELECT id::INTEGER, name::TEXT FROM all_users
EXCEPT
SELECT id::INTEGER, name::TEXT FROM deleted_users
-- inferred: tuple[int, str]
```

## Simple CASE expression

```sql
SELECT
    id::INTEGER,
    CASE
        WHEN score > 90 THEN 'A'
        WHEN score > 80 THEN 'B'
        ELSE 'C'
    END::TEXT
FROM students
-- inferred: tuple[int, str]
```

## CASE with aggregation

```sql
SELECT
    category::TEXT,
    SUM(CASE WHEN active THEN 1 ELSE 0 END)::INTEGER
FROM products
GROUP BY category
-- inferred: tuple[str, int]
```

## COALESCE function

```sql
SELECT
    id::INTEGER,
    COALESCE(nickname, username)::TEXT,
    COALESCE(balance, 0.0)::FLOAT
FROM users
-- inferred: tuple[int, str, float]
```

## NULLIF function

```sql
SELECT
    id::INTEGER,
    NULLIF(status, 'deleted')::TEXT
FROM records
-- inferred: tuple[int, str]
```

## CONCAT function

```sql
SELECT
    id::INTEGER,
    CONCAT(first_name, ' ', last_name)::TEXT
FROM users
-- inferred: tuple[int, str]
```

## SUBSTRING function

```sql
SELECT
    id::INTEGER,
    SUBSTRING(email FROM 1 FOR 10)::TEXT
FROM users
-- inferred: tuple[int, str]
```

## STRING_AGG function

```sql
SELECT
    user_id::INTEGER,
    STRING_AGG(tag, ', ')::TEXT
FROM user_tags
GROUP BY user_id
-- inferred: tuple[int, str]
```

## SELECT * without type casts

```sql
SELECT * FROM users
-- inferred: tuple[str]
```

## SELECT without FROM clause

```sql
SELECT 1::INTEGER, 'hello'::TEXT
-- inferred: tuple[int, str]
```

## SELECT DISTINCT

```sql
SELECT DISTINCT
    category::TEXT,
    status::TEXT
FROM products
-- inferred: tuple[str, str]
```

## SELECT with LIMIT and OFFSET

```sql
SELECT
    id::INTEGER,
    name::TEXT
FROM users
ORDER BY name
LIMIT 10 OFFSET 20
-- inferred: tuple[int, str]
```

## EXISTS subquery

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT
FROM users u
WHERE EXISTS (
    SELECT 1 FROM posts WHERE user_id = u.id
)
-- inferred: tuple[int, str]
```

## IN subquery

```sql
SELECT
    id::INTEGER,
    name::TEXT
FROM users
WHERE id IN (SELECT user_id FROM active_sessions)
-- inferred: tuple[int, str]
```

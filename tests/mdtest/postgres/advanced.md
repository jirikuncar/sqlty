# PostgreSQL Advanced Queries

```toml
[tool.sqlty]
dialect = "postgres"
```

## Window function — ROW_NUMBER

```sql
SELECT
    id::INTEGER,
    name::TEXT,
    ROW_NUMBER() OVER (ORDER BY created_at)::INTEGER
FROM users
-- inferred: tuple[int, str, int]
```

## Window function — running total

```sql
SELECT
    id::INTEGER,
    amount::FLOAT,
    SUM(amount) OVER (ORDER BY created_at)::FLOAT
FROM transactions
-- inferred: tuple[int, float, float]
```

## LATERAL join

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    recent.total::FLOAT,
    recent.created_at::TIMESTAMP
FROM users u
CROSS JOIN LATERAL (
    SELECT total, created_at
    FROM orders
    WHERE user_id = u.id
    ORDER BY created_at DESC
    LIMIT 1
) recent
-- inferred: tuple[int, str, float, datetime.datetime]
```

## UNION

```sql
SELECT id::INTEGER, name::TEXT, 'user'::TEXT as type FROM users
UNION
SELECT id::INTEGER, name::TEXT, 'admin'::TEXT as type FROM admins
-- inferred: tuple[int, str, str]
```

## UNION ALL

```sql
SELECT id::INTEGER, email::TEXT FROM users
UNION ALL
SELECT id::INTEGER, email::TEXT FROM archived_users
-- inferred: tuple[int, str]
```

## INTERSECT

```sql
SELECT user_id::INTEGER FROM orders WHERE total > 100
INTERSECT
SELECT user_id::INTEGER FROM premium_users
-- inferred: tuple[int]
```

## EXCEPT

```sql
SELECT id::INTEGER FROM users
EXCEPT
SELECT user_id::INTEGER FROM banned_users
-- inferred: tuple[int]
```

## Complex real-world query — user dashboard

```sql
WITH user_stats AS (
    SELECT
        user_id,
        COUNT(*)::INTEGER as order_count,
        SUM(total)::FLOAT as total_spent,
        MAX(created_at)::TIMESTAMP as last_order
    FROM orders
    GROUP BY user_id
)
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    u.email::TEXT,
    u.created_at::TIMESTAMP,
    COALESCE(s.order_count, 0)::INTEGER,
    COALESCE(s.total_spent, 0.0)::FLOAT,
    s.last_order::TIMESTAMP,
    u.active::BOOLEAN
FROM users u
LEFT JOIN user_stats s ON u.id = s.user_id
ORDER BY s.total_spent DESC NULLS LAST
-- inferred: tuple[int, str, str, datetime.datetime, int, float, datetime.datetime | None, bool]
```

## ARRAY of a subquery, GROUPING, COLLATE and OVERLAY

`ARRAY(SELECT ...)` is an array of the subquery's column, empty rather than
NULL over no rows. `GROUPING` is a non-null `int`, `COLLATE` keeps its
operand's type, and `OVERLAY` is text.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, nick TEXT, tags TEXT[]);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT ARRAY(SELECT id FROM users) AS ids, ARRAY(SELECT 'x') AS xs
-- columns: ids: list[int] (ARRAY<INT>)
-- columns: xs: list[str] (ARRAY<TEXT>)
```

```sql
SELECT nick, GROUPING(nick) AS g, count(*) AS n FROM users GROUP BY ROLLUP(nick)
-- columns: nick: str | None (TEXT)
-- columns: g: int (INT)
-- columns: n: int (BIGINT)
```

```sql
SELECT name COLLATE "C", nick COLLATE "C" AS n FROM users
-- columns: name: str (TEXT)
-- columns: n: str | None (TEXT)
```

```sql
SELECT OVERLAY(name PLACING 'x' FROM 1 FOR 2) AS a, OVERLAY(name PLACING nick FROM 1) AS b FROM users
-- columns: a: str (TEXT)
-- columns: b: str | None (TEXT)
```

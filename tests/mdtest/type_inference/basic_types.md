# Type Inference: Basic Types

## Integer types

```sql
SELECT id::INTEGER FROM users
-- inferred: tuple[int]
```

## Multiple integer subtypes

All integer-like SQL types map to Python `int`:

```sql
SELECT a::INTEGER, b::BIGINT, c::SMALLINT, d::TINYINT FROM t
-- inferred: tuple[int, int, int, int]
```

## Text types

```sql
SELECT name::TEXT, email::VARCHAR, code::CHAR FROM users
-- inferred: tuple[str, str, str]
```

## Float types

```sql
SELECT price::FLOAT, score::DOUBLE, rating::DECIMAL FROM products
-- inferred: tuple[float, float, decimal.Decimal]
```

## Boolean type

```sql
SELECT active::BOOLEAN FROM users
-- inferred: tuple[bool]
```

## Mixed types in one query

```sql
SELECT
    id::INTEGER,
    name::TEXT,
    price::FLOAT,
    active::BOOLEAN,
    COUNT(*)::INTEGER
FROM products
-- inferred: tuple[int, str, float, bool, int]
```

## No explicit casts leaves the row type unknown

```sql
SELECT id, name FROM users
-- inferred: None
```

## Single column returns single-element tuple

```sql
SELECT COUNT(*) FROM users
-- inferred: tuple[int]
```

## Invalid SQL returns None

```sql
INVALID SQL QUERY
-- inferred: None
```

## Non-SELECT query returns None

```sql
INSERT INTO users VALUES (1, 'test')
-- inferred: None
```

## RETURNING clause

```sql
UPDATE users SET name = 'test' RETURNING id::INTEGER, name::TEXT
-- inferred: tuple[int, str]
```

## Datetime types

```sql
SELECT
    birth_date::DATE,
    created_at::TIMESTAMP,
    updated_at::TIMESTAMPTZ,
    event_time::DATETIME,
    scheduled::TIME,
    notify_at::TIMETZ,
    duration::INTERVAL
FROM events
-- inferred: tuple[datetime.date, datetime.datetime, datetime.datetime, datetime.datetime, datetime.time, datetime.time, datetime.timedelta]
```

## UUID type

```sql
SELECT user_id::UUID FROM users
-- inferred: tuple[uuid.UUID]
```

## Collection types

```sql
SELECT tags::INT[], metadata::JSON, extra::JSONB FROM t
-- inferred: tuple[list[int], typing.Any, typing.Any]
```

## PostgreSQL dialect with named parameters

```toml
[tool.sqlty]
dialect = "postgres"
```

```sql
SELECT id::INTEGER FROM users WHERE id = :id
-- inferred: tuple[int]
```

## MySQL dialect with CAST

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(id AS SIGNED) FROM users
-- inferred: tuple[int]
```

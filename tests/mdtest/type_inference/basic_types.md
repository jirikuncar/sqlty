# Type Inference: Basic Types

## Integer types

```sql
SELECT id::INTEGER FROM users
```

```python
# inferred: tuple[int]
```

## Multiple integer subtypes

All integer-like SQL types map to Python `int`:

```sql
SELECT a::INTEGER, b::BIGINT, c::SMALLINT, d::TINYINT FROM t
```

```python
# inferred: tuple[int, int, int, int]
```

## Text types

```sql
SELECT name::TEXT, email::VARCHAR, code::CHAR FROM users
```

```python
# inferred: tuple[str, str, str]
```

## Float types

```sql
SELECT price::FLOAT, score::DOUBLE, rating::DECIMAL FROM products
```

```python
# inferred: tuple[float, float, float]
```

## Boolean type

```sql
SELECT active::BOOLEAN FROM users
```

```python
# inferred: tuple[bool]
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
```

```python
# inferred: tuple[int, str, float, bool, int]
```

## No explicit casts defaults to str

```sql
SELECT id, name FROM users
```

```python
# inferred: tuple[str, str]
```

## Single column returns single-element tuple

```sql
SELECT COUNT(*) FROM users
```

```python
# inferred: tuple[int]
```

## Invalid SQL returns None

```sql
INVALID SQL QUERY
```

```python
# inferred: None
```

## Non-SELECT query returns None

```sql
INSERT INTO users VALUES (1, 'test')
```

```python
# inferred: None
```

## RETURNING clause

```sql
UPDATE users SET name = 'test' RETURNING id::INTEGER, name::TEXT
```

```python
# inferred: tuple[int, str]
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
```

```python
# inferred: tuple[datetime.date, datetime.datetime, datetime.datetime, datetime.datetime, datetime.time, datetime.time, datetime.timedelta]
```

## UUID type

```sql
SELECT user_id::UUID FROM users
```

```python
# inferred: tuple[uuid.UUID]
```

## Collection types

```sql
SELECT tags::ARRAY, metadata::JSON, extra::JSONB FROM t
```

```python
# inferred: tuple[list, dict, dict]
```

## PostgreSQL dialect with named parameters

```toml
[sqlty]
dialect = "postgres"
```

```sql
SELECT id::INTEGER FROM users WHERE id = :id
```

```python
# inferred: tuple[int]
```

## MySQL dialect with CAST

```toml
[sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(id AS SIGNED) FROM users
```

```python
# inferred: tuple[int]
```

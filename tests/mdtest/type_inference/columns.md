# Column Analysis

## Simple SELECT without type casts

```sql
SELECT id, name FROM users
-- columns: id: None (None)
-- columns: name: None (None)
```

## SELECT with explicit type casts

```sql
SELECT id::INTEGER, name::TEXT FROM users
-- columns: id: int (INT)
-- columns: name: str (TEXT)
```

## SELECT with COUNT aggregate

```sql
SELECT COUNT(*) FROM users
-- columns: count: int (BIGINT)
```

## SELECT with various type casts

```sql
SELECT
    id::INTEGER,
    name::TEXT,
    price::FLOAT,
    active::BOOLEAN,
    COUNT(*)::INTEGER
FROM products
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: price: float (DOUBLE)
-- columns: active: bool (BOOLEAN)
-- columns: count: int (INT)
```

## Return type generation for multiple columns

```sql
SELECT id::INTEGER, name::TEXT, score::FLOAT FROM users
-- inferred: tuple[int, str, float]
```

## Invalid SQL returns None

```sql
INVALID SQL QUERY
-- columns: None
```

## Non-SELECT query returns None

```sql
INSERT INTO users VALUES (1, 'test')
-- columns: None
```

## SQL to Python type mapping

```sql
SELECT id::INTEGER FROM users
-- columns: id: int (INT)
```

## SQL to Python type mapping (2)

```sql
SELECT id::BIGINT FROM users
-- columns: id: int (BIGINT)
```

## SQL to Python type mapping (3)

```sql
SELECT id::SMALLINT FROM users
-- columns: id: int (SMALLINT)
```

## SQL to Python type mapping (4)

```sql
SELECT id::FLOAT FROM users
-- columns: id: float (DOUBLE)
```

## SQL to Python type mapping (5)

```sql
SELECT id::DOUBLE FROM users
-- columns: id: float (DOUBLE)
```

## SQL to Python type mapping (6)

```sql
SELECT id::TEXT FROM users
-- columns: id: str (TEXT)
```

## SQL to Python type mapping (7)

```sql
SELECT id::VARCHAR FROM users
-- columns: id: str (VARCHAR)
```

## SQL to Python type mapping (8)

```sql
SELECT id::BOOLEAN FROM users
-- columns: id: bool (BOOLEAN)
```

## SQL to Python type mapping (9)

```sql
SELECT id::DATE FROM users
-- columns: id: datetime.date (DATE)
```

## SQL to Python type mapping (10)

```sql
SELECT id::TIMESTAMP FROM users
-- columns: id: datetime.datetime (TIMESTAMP)
```

## SQL to Python type mapping (11)

```sql
SELECT id::TIMESTAMPTZ FROM users
-- columns: id: datetime.datetime (TIMESTAMPTZ)
```

## SQL to Python type mapping (12)

```sql
SELECT id::DATETIME FROM users
-- columns: id: datetime.datetime (DATETIME)
```

## SQL to Python type mapping (13)

```sql
SELECT id::TIME FROM users
-- columns: id: datetime.time (TIME)
```

## SQL to Python type mapping (14)

```sql
SELECT id::TIMETZ FROM users
-- columns: id: datetime.time (TIMETZ)
```

## SQL to Python type mapping (15)

```sql
SELECT id::INTERVAL FROM users
-- columns: id: datetime.timedelta (INTERVAL)
```

## SQL to Python type mapping (16)

```sql
SELECT id::UUID FROM users
-- columns: id: uuid.UUID (UUID)
```

## PostgreSQL dialect

```sql
SELECT id::INTEGER FROM users WHERE id = :id
-- columns: id: int (INT)
```

## MySQL dialect

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(id AS SIGNED) FROM users
-- columns: id: int (BIGINT)
```

## UUID type mapping from SQL to Python

```sql
SELECT user_id::UUID FROM users
-- columns: user_id: uuid.UUID (UUID)
```

## UUID in query with multiple types

```sql
SELECT id::INTEGER, user_id::UUID, name::TEXT FROM users
-- columns: id: int (INT)
-- columns: user_id: uuid.UUID (UUID)
-- columns: name: str (TEXT)
```

## DATE type mapping from SQL to Python

```sql
SELECT birth_date::DATE FROM users
-- columns: birth_date: datetime.date (DATE)
```

## TIMESTAMP type mapping from SQL to Python

```sql
SELECT created_at::TIMESTAMP FROM users
-- columns: created_at: datetime.datetime (TIMESTAMP)
```

## TIMESTAMPTZ type mapping from SQL to Python

```sql
SELECT updated_at::TIMESTAMPTZ FROM users
-- columns: updated_at: datetime.datetime (TIMESTAMPTZ)
```

## INTERVAL type mapping from SQL to Python

```sql
SELECT duration::INTERVAL FROM events
-- columns: duration: datetime.timedelta (INTERVAL)
```

## DATETIME type mapping from SQL to Python

```sql
SELECT created_at::DATETIME FROM events
-- columns: created_at: datetime.datetime (DATETIME)
```

## TIMETZ type mapping from SQL to Python

```sql
SELECT scheduled_time::TIMETZ FROM events
-- columns: scheduled_time: datetime.time (TIMETZ)
```

## Datetime types in query with multiple types

```sql
SELECT
    id::INTEGER,
    name::TEXT,
    birth_date::DATE,
    created_at::TIMESTAMP,
    scheduled_time::TIME,
    duration::INTERVAL
FROM users
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: birth_date: datetime.date (DATE)
-- columns: created_at: datetime.datetime (TIMESTAMP)
-- columns: scheduled_time: datetime.time (TIME)
-- columns: duration: datetime.timedelta (INTERVAL)
```

## TIME type mapping from SQL to Python

```sql
SELECT scheduled_time::TIME FROM events
-- columns: scheduled_time: datetime.time (TIME)
```

## Return type generation with datetime types

```sql
SELECT id::INTEGER, created_at::TIMESTAMP, birth_date::DATE FROM users
-- inferred: tuple[int, datetime.datetime, datetime.date]
```

## String and position functions

```sql
SELECT TRIM(name), SUBSTRING(name FROM 1 FOR 2), POSITION('a' IN name) FROM users
-- columns: btrim: str (None)
-- columns: substring: str (None)
-- columns: position: int (None)
```

## Unaliased function names

Unaliased columns are named as PostgreSQL does: after the column (also under a
cast) or the function, else `?column?`. Alias columns that a row type matches
by name.

```sql
SELECT NULLIF(a::INTEGER, 0), my_fn(b), COUNT(*), count(c), LOWER(d), json_build_object('k', 1), NOT e, -f, -1 FROM t
-- columns: nullif: int (INT)
-- columns: my_fn: None (None)
-- columns: count: int (BIGINT)
-- columns: count: int (BIGINT)
-- columns: lower: None (None)
-- columns: json_build_object: typing.Any (JSON)
-- columns: ?column?: bool (BOOLEAN)
-- columns: ?column?: None (None)
-- columns: ?column?: int (None)
```

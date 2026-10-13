# PostgreSQL function return types

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW(),
    big BIGINT,
    small SMALLINT,
    ratio DOUBLE PRECISION,
    valid tstzrange,
    day daterange,
    tags TEXT[]
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Built-in functions

```sql
SELECT
    current_setting('timezone') AS tz,
    left(name, 1) AS initial,
    date_trunc('day', created_at) AS day,
    greatest(id, 0) AS g
FROM users
-- inferred: tuple[str, str, datetime.datetime, int]
```

## Functions returning void

```sql
SELECT pg_advisory_xact_lock(%(key)s)
-- inferred: tuple[None]
```

## Boolean and fixed-type functions

```sql
SELECT pg_try_advisory_lock(1) AS a, gen_random_uuid() AS b, strpos(name, 'a') AS c, random() AS d FROM users
-- columns: a: bool (BOOLEAN)
-- columns: b: uuid.UUID (UUID)
-- columns: c: int (INT)
-- columns: d: float (DOUBLE)
```

## SUM widens like PostgreSQL

```sql
SELECT sum(small) AS a, sum(id) AS b, sum(big) AS c, sum(ratio) AS d FROM users
-- columns: a: int (BIGINT)
-- columns: b: int (BIGINT)
-- columns: c: float (DECIMAL)
-- columns: d: float (DOUBLE)
```

## Range bounds are the element type

`lower` and `upper` stay `TEXT` for anything that is not a range.

```sql
SELECT lower(valid) AS a, upper(valid) AS b, upper(day) AS c, lower(name) AS d FROM users
-- columns: a: datetime.datetime (TIMESTAMPTZ)
-- columns: b: datetime.datetime (TIMESTAMPTZ)
-- columns: c: datetime.date (DATE)
-- columns: d: str (TEXT)
```

## Array functions

```sql
SELECT unnest(tags) AS a, array_agg(id) AS b, array_length(tags, 1) AS c FROM users
-- columns: a: str (TEXT)
-- columns: b: list (ARRAY<INT>)
-- columns: c: int (INT)
```

## Rounding follows the argument

```sql
SELECT round(ratio) AS a, date_trunc('month', created_at::DATE) AS b, avg(ratio) AS c FROM users
-- columns: a: float (DOUBLE)
-- columns: b: datetime.datetime (TIMESTAMPTZ)
-- columns: c: float (DOUBLE)
```

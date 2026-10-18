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
-- inferred: tuple[str, str, datetime.datetime | None, int]
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

## random with bounds

PostgreSQL 17's `random(min, max)` returns the type of its bounds.

```sql
SELECT random(1, 6) AS a, random(1, big) AS b, random(0.0, 1.5) AS c FROM users
-- columns: a: int (INT)
-- columns: b: int | None (BIGINT)
-- columns: c: decimal.Decimal (DECIMAL)
```

## SUM widens like PostgreSQL

```sql
SELECT sum(small) AS a, sum(id) AS b, sum(big) AS c, sum(ratio) AS d FROM users
-- columns: a: int | None (BIGINT)
-- columns: b: int | None (BIGINT)
-- columns: c: decimal.Decimal | None (DECIMAL)
-- columns: d: float | None (DOUBLE)
```

## Range bounds are the element type

`lower` and `upper` stay `TEXT` for anything that is not a range.

```sql
SELECT lower(valid) AS a, upper(valid) AS b, upper(day) AS c, lower(name) AS d FROM users
-- columns: a: datetime.datetime | None (TIMESTAMPTZ)
-- columns: b: datetime.datetime | None (TIMESTAMPTZ)
-- columns: c: datetime.date | None (DATE)
-- columns: d: str (TEXT)
```

## Array functions

```sql
SELECT unnest(tags) AS a, array_agg(id) AS b, array_length(tags, 1) AS c FROM users
-- columns: a: str | None (TEXT)
-- columns: b: list[int] | None (ARRAY<INT>)
-- columns: c: int | None (INT)
```

## Rounding follows the argument

```sql
SELECT round(ratio) AS a, date_trunc('month', created_at::DATE) AS b, avg(ratio) AS c FROM users
-- columns: a: float | None (DOUBLE)
-- columns: b: datetime.datetime | None (TIMESTAMPTZ)
-- columns: c: float | None (DOUBLE)
```

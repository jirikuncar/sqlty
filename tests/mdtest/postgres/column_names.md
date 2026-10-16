# PostgreSQL default column names

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Function and operator columns

```sql
SELECT count(*), max(id), now(), id + 1 FROM users
-- columns: count: int (BIGINT)
-- columns: max: int | None (INT)
-- columns: now: datetime.datetime (TIMESTAMPTZ)
-- columns: ?column?: int (INT)
```

## Columns, casts and keywords

A column keeps its name under a cast, and `lower` is the function's name.

```sql
SELECT id::BIGINT, users.name, lower(email), COALESCE(email, 'x'), CASE WHEN id > 1 THEN 1 END, EXISTS (SELECT 1), 'a', TRUE FROM users
-- columns: id: int (BIGINT)
-- columns: name: str (TEXT)
-- columns: lower: str | None (TEXT)
-- columns: coalesce: str (VARCHAR(255))
-- columns: case: typing.Literal[1] | None (None)
-- columns: exists: bool (BOOLEAN)
-- columns: ?column?: typing.Literal['a'] (None)
-- columns: ?column?: bool (None)
```

## Cast targets

A cast names its column after the value when the value has a name, and after the
target type otherwise. `TRUE` is a constant, not a cast, so it is `?column?`; `TRUE::bool` is `bool`.

```sql
SELECT id::INT, 1::INT, 'a'::TEXT, 'a'::TEXT::INT, id::VARCHAR(5), 1::DECIMAL(5, 2), 1::DOUBLE PRECISION, 1::REAL, '1 day'::INTERVAL, now()::DATE FROM users
-- columns: id: int (INT)
-- columns: int4: int (INT)
-- columns: text: str (TEXT)
-- columns: int4: int (INT)
-- columns: id: str (VARCHAR(5))
-- columns: numeric: decimal.Decimal (DECIMAL(5, 2))
-- columns: float8: float (DOUBLE)
-- columns: float4: float (FLOAT)
-- columns: interval: datetime.timedelta (INTERVAL)
-- columns: now: datetime.date (DATE)
```

## Keywords with a name

```sql
SELECT ROW(1, 2), TRIM(LEADING 'x' FROM name), TRIM(TRAILING 'x' FROM name), TRIM(name), EXTRACT(YEAR FROM created_at), ARRAY[1], -id, id IS NULL, FALSE FROM users
-- columns: row: tuple[typing.Any, ...] (RECORD)
-- columns: ltrim: str (TEXT)
-- columns: rtrim: str (TEXT)
-- columns: btrim: str (TEXT)
-- columns: extract: decimal.Decimal | None (DECIMAL)
-- columns: array: list[int] (ARRAY<INT>)
-- columns: ?column?: int (INT)
-- columns: ?column?: bool (BOOLEAN)
-- columns: ?column?: bool (None)
```

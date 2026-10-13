# PostgreSQL Type Coverage

All 27 SQL types mapped to Python types. Every type must appear at least once.

```toml
[tool.sqlty]
dialect = "postgres"
```

## Integer family

```sql
SELECT
    a::INT,
    b::INTEGER,
    c::BIGINT,
    d::SMALLINT,
    e::TINYINT
FROM t
-- inferred: tuple[int, int, int, int, int]
```

## Float family

```sql
SELECT
    a::FLOAT,
    b::DOUBLE,
    c::REAL,
    d::DECIMAL,
    e::NUMERIC
FROM t
-- inferred: tuple[float, float, float, decimal.Decimal, decimal.Decimal]
```

## String family

```sql
SELECT
    a::TEXT,
    b::VARCHAR,
    c::CHAR,
    d::STRING
FROM t
-- inferred: tuple[str, str, str, str]
```

## Parameterized string types

VARCHAR(N) and CHAR(N) strip the parameter:

```sql
SELECT name::VARCHAR(255), code::CHAR(10) FROM t
-- inferred: tuple[str, str]
```

## Boolean family

```sql
SELECT a::BOOLEAN, b::BOOL FROM t
-- inferred: tuple[bool, bool]
```

## Date and time family

```sql
SELECT
    a::DATE,
    b::TIMESTAMP,
    c::TIMESTAMPTZ,
    d::DATETIME,
    e::TIME,
    f::TIMETZ,
    g::INTERVAL
FROM t
-- inferred: tuple[datetime.date, datetime.datetime, datetime.datetime, datetime.datetime, datetime.time, datetime.time, datetime.timedelta]
```

## UUID type

```sql
SELECT id::UUID FROM t
-- inferred: tuple[uuid.UUID]
```

## Collection types

```sql
SELECT tags::INT[], metadata::JSON, extra::JSONB FROM t
-- inferred: tuple[list[int], typing.Any, typing.Any]
```

## Fixed-size array casts

PostgreSQL reads `x::INT[3]` as a cast (the size is ignored), not as a subscript.

```sql
SELECT a::INT[3], b::TEXT[2][2], c::INT ARRAY[3] FROM t
-- inferred: tuple[list[int], list[list[str]], list[int]]
```

## Bare ARRAY cast is invalid

PostgreSQL needs an element type: `INT[]` or `INT ARRAY`, never plain `ARRAY`.

```sql
SELECT tags::ARRAY FROM t
-- inferred: None
```

## MySQL-only types are invalid

This file uses the default PostgreSQL dialect. Outside MySQL, `SET(...)` and
`UNSIGNED INTEGER` are parse errors, so nothing is inferred.

```sql
SELECT CAST(flags AS SET('x', 'y')), CAST(n AS UNSIGNED INTEGER) FROM t
-- inferred: None
```

## All 27 types in one query

Proves every mapped type is exercised:

```sql
SELECT
    c01::INT, c02::INTEGER, c03::BIGINT, c04::SMALLINT, c05::TINYINT,
    c06::FLOAT, c07::DOUBLE, c08::REAL, c09::DECIMAL, c10::NUMERIC,
    c11::TEXT, c12::VARCHAR, c13::CHAR, c14::STRING,
    c15::BOOLEAN, c16::BOOL,
    c17::DATE, c18::TIMESTAMP, c19::TIMESTAMPTZ, c20::DATETIME,
    c21::TIME, c22::TIMETZ, c23::INTERVAL,
    c24::INT[], c25::JSON, c26::JSONB,
    c27::UUID
FROM t
-- inferred: tuple[int, int, int, int, int, float, float, float, decimal.Decimal, decimal.Decimal, str, str, str, str, bool, bool, datetime.date, datetime.datetime, datetime.datetime, datetime.datetime, datetime.time, datetime.time, datetime.timedelta, list[int], typing.Any, typing.Any, uuid.UUID]
```

## Unknown type defaults to str

```sql
SELECT value::CUSTOM_TYPE FROM t
-- inferred: tuple[str]
```

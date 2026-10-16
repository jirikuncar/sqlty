# PostgreSQL common types

`CASE`, `COALESCE`, `GREATEST`, `LEAST`, `VALUES`, `ARRAY[...]` and set operations
resolve one type for their inputs as PostgreSQL does: untyped literals yield, numbers
promote to the wider type and a mix of integers and decimals is `numeric`.

`schema.sql`:

```sql
CREATE TABLE t (
    i INTEGER NOT NULL,
    b BIGINT NOT NULL,
    s SMALLINT NOT NULL,
    n NUMERIC(10, 2) NOT NULL,
    r REAL NOT NULL,
    d DOUBLE PRECISION NOT NULL,
    tx TEXT NOT NULL,
    v VARCHAR(10) NOT NULL,
    ts TIMESTAMP NOT NULL,
    tz TIMESTAMPTZ NOT NULL,
    dt DATE NOT NULL,
    a INTEGER[] NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## COALESCE

`COALESCE(int_col, 1.5)` is `numeric`, not `integer`.

```sql
SELECT COALESCE(i, 1.5), COALESCE(i, b), COALESCE(s, 1), COALESCE(r, d), COALESCE(r, i) FROM t
-- columns: coalesce: decimal.Decimal (DECIMAL)
-- columns: coalesce: int (BIGINT)
-- columns: coalesce: int (INT)
-- columns: coalesce: float (DOUBLE)
-- columns: coalesce: float (FLOAT)
```

Dates and timestamps meet at the preferred type of their category.

```sql
SELECT COALESCE(dt, ts), COALESCE(ts, tz), COALESCE(dt, tz) FROM t
-- columns: coalesce: datetime.datetime (TIMESTAMP)
-- columns: coalesce: datetime.datetime (TIMESTAMPTZ)
-- columns: coalesce: datetime.datetime (TIMESTAMPTZ)
```

A string literal is `unknown` and takes the other argument's type; `NULL` is ignored.

```sql
SELECT COALESCE(n, '1'), COALESCE(v, 'x'), COALESCE(NULL, 1), COALESCE(a, ARRAY[1]) FROM t
-- columns: coalesce: decimal.Decimal (DECIMAL(10, 2))
-- columns: coalesce: str (VARCHAR(10))
-- columns: coalesce: typing.Literal[1] (None)
-- columns: coalesce: list[int] (ARRAY<INT>)
```

## GREATEST and LEAST

```sql
SELECT GREATEST(i, 1.5), LEAST(i, b), GREATEST(tx, 'a'), LEAST(r, d) FROM t
-- columns: greatest: decimal.Decimal (DECIMAL)
-- columns: least: int (BIGINT)
-- columns: greatest: str (TEXT)
-- columns: least: float (DOUBLE)
```

## CASE

Every branch counts, not only the first.

```sql
SELECT CASE WHEN i > 1 THEN 1 ELSE 2.5 END, CASE WHEN i > 1 THEN i ELSE b END, CASE i WHEN 1 THEN 'a' ELSE tx END, CASE WHEN i > 1 THEN NULL ELSE 1 END FROM t
-- columns: case: decimal.Decimal (DECIMAL)
-- columns: b: int (BIGINT)
-- columns: tx: str (TEXT)
-- columns: case: typing.Literal[1] | None (None)
```

## ARRAY

```sql
SELECT ARRAY[i, 1.5], ARRAY[i, b], ARRAY[tx, 'a'] FROM t
-- columns: array: list[decimal.Decimal] (ARRAY<DECIMAL>)
-- columns: array: list[int] (ARRAY<BIGINT>)
-- columns: array: list[str] (ARRAY<TEXT>)
```

## VALUES

Columns are `column1`, `column2`, ... typed by their common type.

```sql
VALUES (1, 'a'), (2.5, 'b')
-- columns: column1: decimal.Decimal (DECIMAL)
-- columns: column2: typing.Literal['a', 'b'] (None)
```

```sql
VALUES (1, NULL), (2, 'b')
-- columns: column1: typing.Literal[1, 2] (None)
-- columns: column2: typing.Literal['b'] | None (None)
```

```sql
SELECT x, y FROM (VALUES (1, 2), (3, 4)) AS v (x, y)
-- columns: x: typing.Literal[1, 3] (None)
-- columns: y: typing.Literal[2, 4] (None)
```

## Set operations

An untyped literal takes the type of the other branch.

```sql
SELECT i FROM t UNION SELECT 1.5
-- columns: i: decimal.Decimal (DECIMAL)
```

```sql
SELECT n FROM t UNION SELECT '1'
-- columns: n: decimal.Decimal (DECIMAL(10, 2))
```

## Smallint and integer literals

An integer literal is an `integer`, so it widens a `smallint`.

```sql
SELECT s + 1, s + s, s + b FROM t
-- columns: ?column?: int (INT)
-- columns: ?column?: int (SMALLINT)
-- columns: ?column?: int (BIGINT)
```

## Operators

Bitwise operators work on integers; a shift keeps the type of its left operand.

```sql
SELECT i & 3, i | b, i # 1, i << 2, b >> 1, ~i FROM t
-- columns: ?column?: int (INT)
-- columns: ?column?: int (BIGINT)
-- columns: ?column?: int (INT)
-- columns: ?column?: int (INT)
-- columns: ?column?: int (BIGINT)
-- columns: ?column?: int (INT)
```

`||` concatenates arrays, and an element on either side joins the array.

```sql
SELECT a || a, a || 1, 1 || a, a || ARRAY[2] FROM t
-- columns: ?column?: list[int] (ARRAY<INT>)
-- columns: ?column?: list[int] (ARRAY<INT>)
-- columns: ?column?: list[int] (ARRAY<INT>)
-- columns: ?column?: list[int] (ARRAY<INT>)
```

A `real` (`r`) stays `real` only with another `real`; with any other number
PostgreSQL computes in `double precision`.

```sql
SELECT r + r, r + 1, r * i, r + n, r - b FROM t
-- columns: ?column?: float (FLOAT)
-- columns: ?column?: float (DOUBLE)
-- columns: ?column?: float (DOUBLE)
-- columns: ?column?: float (DOUBLE)
-- columns: ?column?: float (DOUBLE)
```

Dates, times and intervals.

```sql
SELECT 1 + dt, dt + '10:00'::time, '10:00'::time + INTERVAL '1 hour', '10:00'::time - '09:00'::time, INTERVAL '1 day' / 2 FROM t
-- columns: ?column?: datetime.date (DATE)
-- columns: ?column?: datetime.datetime (TIMESTAMP)
-- columns: ?column?: datetime.time (TIME)
-- columns: ?column?: datetime.timedelta (INTERVAL)
-- columns: ?column?: datetime.timedelta (INTERVAL)
```

`AT TIME ZONE` flips between `timestamp` and `timestamptz`; `1e3` is a `numeric`.

```sql
SELECT ts AT TIME ZONE 'UTC', tz AT TIME ZONE 'UTC', 1e3 FROM t
-- columns: timezone: datetime.datetime (TIMESTAMPTZ)
-- columns: timezone: datetime.datetime (TIMESTAMP)
-- columns: ?column?: decimal.Decimal (None)
```

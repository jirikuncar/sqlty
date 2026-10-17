# ISO SQL types

Predefined and constructed types of ISO/IEC 9075-2 beyond the common ones. A
type sqlty cannot map leaves its column untyped; the rest of the query is still
typed.

```toml
[tool.sqlty]
dialect = "postgres"
```

## Intervals with a qualifier

Any `INTERVAL` is a `datetime.timedelta`, as psycopg loads it, and its
qualifier or precision is dropped from `sql_type`. A year-month interval has no
exact `timedelta`: psycopg loads `INTERVAL '1 year 2 months'` as
`timedelta(days=425)`, a year as 365 days and a month as 30.

```sql
SELECT CAST(x AS INTERVAL DAY TO SECOND) AS a, CAST(x AS INTERVAL YEAR TO MONTH) AS b, CAST(x AS INTERVAL HOUR) AS c
-- columns: a: datetime.timedelta (INTERVAL)
-- columns: b: datetime.timedelta (INTERVAL)
-- columns: c: datetime.timedelta (INTERVAL)
```

## Large objects

`CHARACTER LARGE OBJECT` is text and `BINARY LARGE OBJECT` binary, like `CLOB`
and `BLOB`.

```sql
SELECT CAST(x AS CHARACTER LARGE OBJECT) AS a, CAST(x AS CLOB) AS b, CAST(x AS BINARY LARGE OBJECT) AS c, CAST(x AS BLOB) AS d
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
-- columns: c: bytes (VARBINARY)
-- columns: d: bytes (VARBINARY)
```

## DECFLOAT

A decimal floating point number is a `decimal.Decimal`, a `float` in SQLite.

```sql
SELECT CAST(x AS DECFLOAT) AS a
-- columns: a: decimal.Decimal (DECFLOAT)
```

## Bit strings

psycopg loads `bit` and `bit varying` as text; MySQL drivers load `BIT(n)` as
bytes (see `tests/mdtest/mysql/types.md`).

```sql
SELECT CAST(x AS BIT(3)) AS a, CAST(x AS BIT VARYING(5)) AS b, CAST(x AS VARBIT) AS c
-- columns: a: str (BIT(3))
-- columns: b: str (VARBIT(5))
-- columns: c: str (VARBIT)
```

## Rows and multisets

`ROW(...)` is a tuple of its field types, as psycopg loads a record, and keeps
its field names in `sql_type`. A `MULTISET` is a list, like an array.

```sql
SELECT CAST(x AS ROW(a INT, b TEXT)) AS a, CAST(x AS INTEGER MULTISET) AS b
-- columns: a: tuple[int, str] (ROW(a INT, b TEXT))
-- columns: b: list[int] (ARRAY<INT>)
```

## Unmapped types

A range is loaded as psycopg's `Range`, which sqlty does not name, so its column
is untyped; the other columns keep their types.

```sql
SELECT CAST(x AS INT4RANGE) AS a, CAST(x AS TSTZRANGE) AS b, 1::INTEGER AS c
-- columns: a: None (INT4RANGE)
-- columns: b: None (TSTZRANGE)
-- columns: c: int (INT)
```

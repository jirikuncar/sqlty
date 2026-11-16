# Datetime Type Inference

## TIMESTAMP infers datetime.datetime

```sql
SELECT created_at::TIMESTAMP FROM events
-- inferred: tuple[datetime.datetime]
```

## DATE infers datetime.date

```sql
SELECT birth_date::DATE FROM users
-- inferred: tuple[datetime.date]
```

## TIME infers datetime.time

```sql
SELECT start_time::TIME FROM schedule
-- inferred: tuple[datetime.time]
```

## INTERVAL infers datetime.timedelta

```sql
SELECT duration::INTERVAL FROM tasks
-- inferred: tuple[datetime.timedelta]
```

## Multiple datetime types

```sql
SELECT created_at::TIMESTAMP, birth_date::DATE, start_time::TIME FROM users
-- inferred: tuple[datetime.datetime, datetime.date, datetime.time]
```

## Typed string literals

A literal prefixed with its type, like `DATE '2024-01-01'`, has that type.

```sql
SELECT DATE '2024-01-01' AS d, TIME '12:00' AS t, TIMESTAMP '2024-01-01 00:00:00' AS ts,
       TIMESTAMPTZ '2024-01-01 00:00:00+00' AS tstz, INTERVAL '1 day' AS i
-- columns: d: datetime.date (DATE)
-- columns: t: datetime.time (TIME)
-- columns: ts: datetime.datetime (TIMESTAMP)
-- columns: tstz: datetime.datetime (TIMESTAMPTZ)
-- columns: i: datetime.timedelta (INTERVAL)
```

## EXTRACT and date_part

Each database gives `EXTRACT` its own result type. A year is never a `date`.

`schema.sql`:

```sql
CREATE TABLE orders (placed_at TIMESTAMP NOT NULL, d DATE NOT NULL);
```

### PostgreSQL

`EXTRACT` returns `numeric` and `date_part` returns `double precision`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS year, date_part('year', placed_at) AS part FROM orders
-- columns: year: decimal.Decimal (DECIMAL)
-- columns: part: float (DOUBLE)
```

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS year FROM orders
-- columns: year: int (BIGINT)
```

### DuckDB

Every part is a `BIGINT`, except `epoch` and `julian`, which are a `DOUBLE`.

```toml
[tool.sqlty]
dialect = "duckdb"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS year, EXTRACT(EPOCH FROM placed_at) AS epoch,
    EXTRACT(JULIAN FROM d) AS julian, date_part('year', placed_at) AS part,
    date_part('epoch', placed_at) AS part_epoch
FROM orders
-- columns: year: int (BIGINT)
-- columns: epoch: float (DOUBLE)
-- columns: julian: float (DOUBLE)
-- columns: part: int (BIGINT)
-- columns: part_epoch: float (DOUBLE)
```

### ClickHouse

`EXTRACT(YEAR FROM x)` is `toYear(x)`, a `UInt16`; the other calendar and clock parts are a
`UInt8`.

```toml
[tool.sqlty]
dialect = "clickhouse"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS year, EXTRACT(MONTH FROM placed_at) AS month FROM orders
-- columns: year: int (USMALLINT)
-- columns: month: int (UTINYINT)
```

### SQLite

SQLite has no `EXTRACT`: `strftime('%Y', x)` gives the year as text.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

```python
from sqlty import SQL

# error: [rejected-query] "SQLite has no EXTRACT; use strftime"
q = SQL("--sql\nSELECT EXTRACT(YEAR FROM placed_at) AS year FROM orders")
r = SQL[tuple[str | None]]("--sql\nSELECT strftime('%Y', placed_at) AS year FROM orders")
```

### Generic SQL

Generic SQL follows PostgreSQL.

```toml
[tool.sqlty]
dialect = "generic"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS year FROM orders
-- columns: year: decimal.Decimal (DECIMAL)
```

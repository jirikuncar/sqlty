# EXTRACT

The type of `EXTRACT(field FROM x)` depends on the dialect, and in ClickHouse and
DuckDB on the field.

`schema.sql`:

```sql
CREATE TABLE orders (id INT PRIMARY KEY, placed_at TIMESTAMP NOT NULL, shipped DATE)
```

## PostgreSQL

`numeric` for every field since PostgreSQL 14; `date_part` stays `double precision`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS y, EXTRACT(EPOCH FROM placed_at) AS e, date_part('year', placed_at) AS p, EXTRACT(DAY FROM shipped) AS d FROM orders
-- columns: y: decimal.Decimal (DECIMAL)
-- columns: e: decimal.Decimal (DECIMAL)
-- columns: p: float (DOUBLE)
-- columns: d: decimal.Decimal | None (DECIMAL)
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS y, EXTRACT(DAY FROM shipped) AS d FROM orders
-- columns: y: int (BIGINT)
-- columns: d: int | None (BIGINT)
```

## DuckDB

`BIGINT`, except `epoch` and `julian`, which are `DOUBLE`.

```toml
[tool.sqlty]
dialect = "duckdb"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS y, EXTRACT(EPOCH FROM placed_at) AS e, EXTRACT(DAY FROM shipped) AS d FROM orders
-- columns: y: int (BIGINT)
-- columns: e: float (DOUBLE)
-- columns: d: int | None (BIGINT)
```

## ClickHouse

`EXTRACT(YEAR FROM x)` is `toYear(x)`, a `UInt16`.

```toml
[tool.sqlty]
dialect = "clickhouse"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS y, EXTRACT(MONTH FROM placed_at) AS m, EXTRACT(EPOCH FROM placed_at) AS e, EXTRACT(CENTURY FROM placed_at) AS c FROM orders
-- columns: y: int (USMALLINT)
-- columns: m: int (UTINYINT)
-- columns: e: int (UINT)
-- columns: c: int (BIGINT)
```

## Generic SQL

Follows PostgreSQL.

```toml
[tool.sqlty]
dialect = "generic"
schema = "schema.sql"
```

```sql
SELECT EXTRACT(YEAR FROM placed_at) AS y FROM orders
-- columns: y: decimal.Decimal (DECIMAL)
```

# Dialect-specific Type Inference

## PostgreSQL integer cast

```toml
[tool.sqlty]
dialect = "postgres"
```

```sql
SELECT id::INTEGER FROM users
-- inferred: tuple[int]
```

## MySQL SIGNED cast

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(id AS SIGNED) FROM users
-- inferred: tuple[int]
```

## SQLite integer

```toml
[tool.sqlty]
dialect = "sqlite"
```

```sql
SELECT CAST(id AS INTEGER) FROM users
-- inferred: tuple[int]
```

## Other dialects

Any dialect [sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs)
knows is parsed with its own rules (`tsql` is `mssql`); unknown names read as
generic SQL. Common type names are spelled the PostgreSQL way.

### BigQuery

```toml
[tool.sqlty]
dialect = "bigquery"
schema = "schema.sql"
```

`schema.sql`:
```sql
CREATE TABLE t (a INT64, b FLOAT64, c BIGNUMERIC, d STRING, e BYTES, f BOOL)
```

```sql
SELECT * FROM t
-- inferred: tuple[int | None, float | None, decimal.Decimal | None, str | None, bytes | None, bool | None]
-- expanded: SELECT a::BIGINT, b::DOUBLE, c::BIGDECIMAL, d::TEXT, e::BINARY, f::BOOLEAN FROM t
```

### Snowflake

```toml
[tool.sqlty]
dialect = "snowflake"
schema = "schema.sql"
```

`schema.sql`:
```sql
CREATE TABLE t (a NUMBER(10,2), b TIMESTAMP_NTZ, c TIMESTAMP_TZ, d TIMESTAMP_LTZ, e VARCHAR)
```

```sql
SELECT * FROM t
-- inferred: tuple[decimal.Decimal | None, datetime.datetime | None, datetime.datetime | None, datetime.datetime | None, str | None]
-- expanded: SELECT a::DECIMAL(10, 2), b::TIMESTAMP, c::TIMESTAMPTZ, d::TIMESTAMPTZ, e::VARCHAR FROM t
```

### ClickHouse

```toml
[tool.sqlty]
dialect = "clickhouse"
schema = "schema.sql"
```

`schema.sql`:
```sql
CREATE TABLE t (a Int8, b Int32, c UInt64, d Float32, e Float64, f String)
```

```sql
SELECT * FROM t
-- inferred: tuple[int | None, int | None, int | None, float | None, float | None, str | None]
-- expanded: SELECT a::TINYINT, b::INT, c::UBIGINT, d::FLOAT, e::DOUBLE, f::TEXT FROM t
```

### T-SQL

```toml
[tool.sqlty]
dialect = "tsql"
schema = "schema.sql"
```

`schema.sql`:
```sql
CREATE TABLE t (a INT, b DATETIME2, c DATETIMEOFFSET, d UNIQUEIDENTIFIER, e NVARCHAR(10))
```

```sql
SELECT * FROM t
-- inferred: tuple[int | None, datetime.datetime | None, datetime.datetime | None, uuid.UUID | None, str | None]
-- expanded: SELECT a::INT, b::TIMESTAMP, c::TIMESTAMPTZ, d::UUID, e::VARCHAR(10) FROM t
```

### Oracle

```toml
[tool.sqlty]
dialect = "oracle"
schema = "schema.sql"
```

`schema.sql`:
```sql
CREATE TABLE t (a NUMBER(10,2), b VARCHAR2(20), c CLOB, d DATE)
```

```sql
SELECT * FROM t
-- inferred: tuple[float | None, str | None, str | None, datetime.date | None]
-- expanded: SELECT a::DECIMAL(10, 2), b::VARCHAR(20), c::TEXT, d::DATE FROM t
```

### Unknown dialect

```toml
[tool.sqlty]
dialect = "nosuch"
```

```sql
SELECT CAST(id AS INTEGER) FROM t
-- inferred: tuple[int]
```

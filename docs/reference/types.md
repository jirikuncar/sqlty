# Type mapping

Each SQL type maps to the Python type the dialect's usual driver returns for it.
The tables below are tests: each query is analyzed when the docs are tested, so
they cannot drift from the engine. The mapping itself lives in
`rust/crates/sqlty-sql/src/analyzer/sql_type.rs`.

| Dialect | Driver the mapping follows |
|---------|----------------------------|
| `postgres` | [psycopg 3](https://www.psycopg.org/psycopg3/docs/basic/adapt.html) |
| `mysql` | PyMySQL and mysqlclient |
| `sqlite` | the standard library's [`sqlite3`](https://docs.python.org/3/library/sqlite3.html) |
| `duckdb` | the [DuckDB Python client](https://duckdb.org/docs/current/clients/python/conversion.html) |

## Deliberate differences

These look like bugs but follow the drivers:

| SQL type | Python type | Why |
|----------|-------------|-----|
| `NUMERIC`, `DECIMAL`, `DECFLOAT` | `decimal.Decimal`; `float` in `sqlite` and `oracle` | SQLite stores them with `NUMERIC` affinity and `sqlite3` returns floats |
| MySQL `TIME` | `datetime.timedelta` | A MySQL `TIME` is a duration, up to 838 hours either way |
| `BIT(n)` | `str`; `bytes` in `mysql` | psycopg loads a bit string as its text, MySQL drivers as bytes |
| `JSON`, `JSONB` | `typing.Any` | Any JSON value can be stored; a column that builds JSON of a known shape is checked by [`json-shape-mismatch`](rules.md#json-shape-mismatch) |
| `INET` | `ipaddress.IPv4Address \| ipaddress.IPv6Address \| ipaddress.IPv4Interface \| ipaddress.IPv6Interface` | psycopg loads an address with a prefix (`1.2.3.4/24`) as an interface |
| `MONEY`, `XML`, `reg*`, `xid`, `tsvector` | `str` | psycopg loads them as text |
| `INTERVAL` with a year or month part | `datetime.timedelta` | psycopg counts a year as 365 days and a month as 30 |
| Ranges, geometric types | unknown | psycopg loads ranges as its own `Range`; [`unknown-type`](rules.md#unknown-type) names them |
| DuckDB `T[3]` | `tuple[T, ...]` | The DuckDB client returns a fixed-size `ARRAY` as a tuple, a `LIST` (`T[]`) as a list |

## PostgreSQL

```toml
[tool.sqlty]
dialect = "postgres"
```

```sql
SELECT CAST(x AS SMALLINT) AS a, CAST(x AS INTEGER) AS b, CAST(x AS BIGINT) AS c, CAST(x AS NUMERIC(10, 2)) AS d, CAST(x AS REAL) AS e, CAST(x AS DOUBLE PRECISION) AS f
-- columns: a: int (SMALLINT)
-- columns: b: int (INT)
-- columns: c: int (BIGINT)
-- columns: d: decimal.Decimal (DECIMAL(10, 2))
-- columns: e: float (FLOAT)
-- columns: f: float (DOUBLE)
```

```sql
SELECT CAST(x AS TEXT) AS a, CAST(x AS VARCHAR(10)) AS b, CAST(x AS CHAR(2)) AS c, CAST(x AS BYTEA) AS d, CAST(x AS BOOLEAN) AS e, CAST(x AS UUID) AS f
-- columns: a: str (TEXT)
-- columns: b: str (VARCHAR(10))
-- columns: c: str (CHAR(2))
-- columns: d: bytes (VARBINARY)
-- columns: e: bool (BOOLEAN)
-- columns: f: uuid.UUID (UUID)
```

```sql
SELECT CAST(x AS DATE) AS a, CAST(x AS TIME) AS b, CAST(x AS TIMETZ) AS c, CAST(x AS TIMESTAMP) AS d, CAST(x AS TIMESTAMPTZ) AS e, CAST(x AS INTERVAL) AS f
-- columns: a: datetime.date (DATE)
-- columns: b: datetime.time (TIME)
-- columns: c: datetime.time (TIMETZ)
-- columns: d: datetime.datetime (TIMESTAMP)
-- columns: e: datetime.datetime (TIMESTAMPTZ)
-- columns: f: datetime.timedelta (INTERVAL)
```

```sql
SELECT CAST(x AS JSONB) AS a, CAST(x AS INET) AS b, CAST(x AS CIDR) AS c, CAST(x AS MONEY) AS d, CAST(x AS BIT(3)) AS e, CAST(x AS INT[]) AS f
-- columns: a: typing.Any (JSONB)
-- columns: b: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: c: ipaddress.IPv4Network | ipaddress.IPv6Network (CIDR)
-- columns: d: str (MONEY)
-- columns: e: str (BIT(3))
-- columns: f: list[int] (ARRAY<INT>)
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(x AS SIGNED) AS a, CAST(x AS UNSIGNED) AS b, CAST(x AS DECIMAL(10, 2)) AS c, CAST(x AS DOUBLE) AS d, CAST(x AS CHAR) AS e, CAST(x AS BINARY) AS f
-- columns: a: int (BIGINT)
-- columns: b: int (UBIGINT)
-- columns: c: decimal.Decimal (DECIMAL(10, 2))
-- columns: d: float (DOUBLE)
-- columns: e: str (CHAR)
-- columns: f: bytes (BINARY)
```

```sql
SELECT CAST(x AS DATE) AS a, CAST(x AS TIME) AS b, CAST(x AS DATETIME) AS c, CAST(x AS JSON) AS d, CAST(x AS YEAR) AS e
-- columns: a: datetime.date (DATE)
-- columns: b: datetime.timedelta (TIME)
-- columns: c: datetime.datetime (DATETIME)
-- columns: d: typing.Any (JSON)
-- columns: e: int (YEAR)
```

## SQLite

A type name SQLite has no mapping for gets the Python type of its
[type affinity](https://www.sqlite.org/datatype3.html#determination_of_column_affinity).

```toml
[tool.sqlty]
dialect = "sqlite"
```

```sql
SELECT CAST(x AS INTEGER) AS a, CAST(x AS REAL) AS b, CAST(x AS TEXT) AS c, CAST(x AS BLOB) AS d, CAST(x AS NUMERIC) AS e, CAST(x AS VARCHAR(10)) AS f
-- columns: a: int (INT)
-- columns: b: float (FLOAT)
-- columns: c: str (TEXT)
-- columns: d: bytes (VARBINARY)
-- columns: e: float (DECIMAL)
-- columns: f: str (VARCHAR(10))
```

## DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
```

```sql
SELECT CAST(x AS HUGEINT) AS a, CAST(x AS UBIGINT) AS b, CAST(x AS INTEGER[]) AS c, CAST(x AS STRUCT(a INT, b VARCHAR)) AS d, CAST(x AS MAP(VARCHAR, INT)) AS e, CAST(x AS UNION(n INT, s VARCHAR)) AS f
-- columns: a: int (INT128)
-- columns: b: int (UBIGINT)
-- columns: c: list[int] (ARRAY<INT>)
-- columns: d: dict[str, int | str] (STRUCT(a INT, b VARCHAR))
-- columns: e: dict[str, int] (MAP(VARCHAR, INT))
-- columns: f: int | str (UNION(n INT, s VARCHAR))
```

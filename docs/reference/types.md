# Type mapping

Each SQL type maps to the Python type the dialect's usual driver returns for it.
The tables below are tests: each query is analyzed when the docs are tested, so
they cannot drift from the engine. The mapping itself lives in
`rust/crates/sqlty-sql/src/analyzer/sql_type.rs`.

| Dialect | Reference driver | Checked by |
|---------|------------------|------------|
| `postgres` | [psycopg 3](https://www.psycopg.org/psycopg3/docs/basic/adapt.html) | `tests/test_driver_values.py`, against PostgreSQL 17 in CI |
| `mysql` | [PyMySQL](https://github.com/PyMySQL/PyMySQL/blob/main/pymysql/converters.py); mysqlclient agrees on every type in the table | `tests/test_driver_values.py`, with `SQLTY_MYSQL_DSN` |
| `sqlite` | the standard library's [`sqlite3`](https://docs.python.org/3/library/sqlite3.html), without `detect_types` | `tests/test_driver_values.py`, in every run |
| `duckdb` | the [DuckDB Python client](https://duckdb.org/docs/current/clients/python/conversion.html) | `tests/test_duckdb_types.py`, in every run |

The reference driver returns the raw value: what a converter above it (an ORM,
`detect_types`, a row factory that decodes JSON) makes of it is not modelled. See
[Drivers and ORMs](../guide/drivers-and-orms.md) for where ORMs differ and what
to annotate.

!!! note "Other PostgreSQL drivers"
    asyncpg returns `json`/`jsonb` as `str`, `bit` as `asyncpg.BitString` and
    `"char"` as `bytes`; psycopg2 returns `bytea` as `memoryview`, and `uuid` and
    `inet` as `str` unless you register their adapters. Annotate those fields
    with the type your driver returns, and suppress the mismatch with
    `# sqlty: ignore[column-type-mismatch]`.

## Deliberate differences

These look like bugs but follow the drivers:

| SQL type | Python type | Why |
|----------|-------------|-----|
| `NUMERIC`, `DECIMAL`, `DECFLOAT` | `decimal.Decimal`; `float` in `sqlite` and `oracle` | SQLite stores them with `NUMERIC` affinity and `sqlite3` returns floats |
| SQLite `DATE`, `TIME`, `TIMESTAMP`, `DATETIME` | `str` | SQLite has no date storage class: `sqlite3` returns the stored text, unless you pass `detect_types` and register converters |
| SQLite `BOOLEAN`, and comparisons | `int` | SQLite stores booleans as the integers 0 and 1 |
| MySQL `TIME` | `datetime.timedelta` | A MySQL `TIME` is a duration, up to 838 hours either way |
| `BIT(n)` | `str`; `bytes` in `mysql` | psycopg loads a bit string as its text, MySQL drivers as bytes |
| `JSON`, `JSONB` | `typing.Any` | Any JSON value can be stored. psycopg returns it parsed, PyMySQL and `sqlite3` as text; `Any` admits both, so code that decodes it keeps its [`json-shape-mismatch`](rules.md#json-shape-mismatch) checks |
| SQLite `UUID` | `str` | SQLite has no UUID storage class: `sqlite3` returns the stored text |
| SQLite `NUMERIC` affinity (`NUMERIC`, `DECIMAL`, unknown names) | `float` | `sqlite3` returns an integral value as `int`, which a type checker accepts for `float`; precision beyond a double is lost on storage, not by sqlty |
| MySQL `SET` | `str` | PyMySQL returns the members as one comma-separated string |
| MySQL `BOOL`, `BOOLEAN` | `bool` | Kept to tell a flag from a number: PyMySQL returns the `TINYINT(1)` it is stored as, an `int`; `TINYINT(1)` itself is `int` |
| MySQL `REAL` | `float` (`DOUBLE`) | `REAL` is `DOUBLE` unless the `REAL_AS_FLOAT` SQL mode is on |
| Type names of other dialects in `postgres` (`BLOB`, `DATETIME`, `TINYINT`) | their PostgreSQL counterparts | The default dialect reads any project's names; sqlty does not check that PostgreSQL has the type, PostgreSQL rejects the query |
| MySQL spatial types (`GEOMETRY`, `POINT`, ...) | `bytes` | PyMySQL returns MySQL's internal binary format |
| `INET` | `ipaddress.IPv4Address \| ipaddress.IPv6Address \| ipaddress.IPv4Interface \| ipaddress.IPv6Interface` | psycopg loads an address with a prefix (`1.2.3.4/24`) as an interface |
| `MONEY`, `XML`, `reg*`, `xid`, `tsvector` | `str` | psycopg loads them as text |
| `INTERVAL` with a year or month part | `datetime.timedelta` | psycopg counts a year as 365 days and a month as 30 |
| Ranges, multiranges | unknown | psycopg loads them as its own `Range` and `Multirange`; [`unknown-type`](rules.md#unknown-type) names them |
| PostgreSQL geometric types (`point`, `box`, ...) | `str` | psycopg has no loader for them and returns their text |
| `DATE 'infinity'`, MySQL `'0000-00-00'` | the column's type | Values outside Python's range: psycopg raises `DataError` for `infinity` (asyncpg and psycopg2 return `date.max`), PyMySQL returns a zero date as `str`, mysqlclient as `None` |
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

```sql
SELECT CAST(x AS DATE) AS a, CAST(x AS TIMESTAMP) AS b, CAST(x AS DATETIME) AS c, CAST(x AS BOOLEAN) AS d, 1 = 1 AS e
-- columns: a: str (DATE)
-- columns: b: str (TIMESTAMP)
-- columns: c: str (DATETIME)
-- columns: d: int (BOOLEAN)
-- columns: e: int (BOOLEAN)
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

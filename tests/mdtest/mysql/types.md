# MySQL Types

## Integer columns

`INT3` and `MIDDLEINT` are aliases for `MEDIUMINT`, and `INT8` for `BIGINT`. A
trailing `SIGNED` is the default and is dropped.

`schema.sql`:
```sql
CREATE TABLE items (
    a TINYINT,
    b MEDIUMINT,
    c INT3,
    d MIDDLEINT,
    e INT8,
    f INT(11) SIGNED,
    g YEAR
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: tuple[int | None, int | None, int | None, int | None, int | None, int | None, int | None]
-- columns: a: int | None (TINYINT)
-- columns: b: int | None (MEDIUMINT)
-- columns: c: int | None (MEDIUMINT)
-- columns: d: int | None (MEDIUMINT)
-- columns: e: int | None (BIGINT)
-- columns: f: int | None (INT(11))
-- columns: g: int | None (YEAR)
```

## Unsigned columns

`UNSIGNED` columns are spelled with a `U` prefix (`INT UNSIGNED` is `UINT`), and
`FLOAT UNSIGNED` becomes `UDOUBLE`. They map to the same Python types as their
signed forms.

`schema.sql`:
```sql
CREATE TABLE items (
    a INT UNSIGNED,
    b BIGINT(20) UNSIGNED,
    c FLOAT UNSIGNED,
    d DECIMAL(10,2) UNSIGNED
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: tuple[int | None, int | None, float | None, decimal.Decimal | None]
-- columns: a: int | None (UINT)
-- columns: b: int | None (UBIGINT(20))
-- columns: c: float | None (UDOUBLE)
-- columns: d: decimal.Decimal | None (UDECIMAL(10, 2))
```

## Temporal and other columns

MySQL `TIMESTAMP` is stored in UTC, so it reads as `TIMESTAMPTZ`. Blob and text
variants collapse to `BLOB` and `TEXT`. `ENUM` and `SET` values are upper-cased
in the expansion; an `ENUM` column is a `typing.Literal` of its labels.
Unmapped types such as `BIT`, `SET` and spatial `POINT` are unknown, so a
query selecting one has no row type.

`schema.sql`:
```sql
CREATE TABLE items (
    a DOUBLE,
    b BIT(8),
    c DATETIME(6),
    d TIMESTAMP,
    e ENUM('x', 'y'),
    f SET('x', 'y'),
    g JSON,
    h MEDIUMBLOB,
    i LONGTEXT,
    j POINT
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: None
-- columns: a: float | None (DOUBLE)
-- columns: b: None (BIT(8))
-- columns: c: datetime.datetime | None (DATETIME(6))
-- columns: d: datetime.datetime | None (TIMESTAMPTZ)
-- columns: e: typing.Literal['x', 'y'] | None (ENUM('X', 'Y'))
-- columns: f: None (SET('X', 'Y'))
-- columns: g: typing.Any (JSON)
-- columns: h: bytes | None (BLOB)
-- columns: i: str | None (TEXT)
-- columns: j: None (POINT)
```

```sql
SELECT a, c, d, e, g, h, i FROM items
-- inferred: tuple[float | None, datetime.datetime | None, datetime.datetime | None, typing.Literal['x', 'y'] | None, typing.Any, bytes | None, str | None]
```

## Unsigned NUMERIC and FIXED are rejected

`UNSIGNED` applies to `DECIMAL` (or `DEC`), not to its synonyms `NUMERIC` and
`FIXED`, so nothing is inferred.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(a AS NUMERIC(8,3) UNSIGNED), CAST(b AS FIXED UNSIGNED) FROM t
-- inferred: None
```

## Integer CASTs

`SIGNED` and `UNSIGNED` (with or without `INTEGER`) are MySQL's integer cast
targets.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(a AS SIGNED), CAST(b AS UNSIGNED INTEGER), CAST(c AS YEAR) FROM t
-- inferred: tuple[int, int, int]
```

## Multi-valued index CAST to an array

MySQL's only array type is the `... ARRAY` cast used by multi-valued indexes.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(doc->'$.zips' AS UNSIGNED ARRAY) FROM t
-- inferred: tuple[list[int] | None]
```

## Bracketed array types are not MySQL

PostgreSQL's `INT[]` is a parse error in MySQL, so nothing is inferred.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(tags AS INT[]) FROM t
-- inferred: None
```

## INT3, MIDDLEINT and ZEROFILL

`INT3 UNSIGNED` and `MIDDLEINT UNSIGNED` are `UMEDIUMINT`. `ZEROFILL` implies
`UNSIGNED`.

`schema.sql`:
```sql
CREATE TABLE items (
    a INT3 UNSIGNED,
    b MIDDLEINT(8) UNSIGNED,
    c INT(5) ZEROFILL,
    d BIGINT UNSIGNED ZEROFILL
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: tuple[int | None, int | None, int | None, int | None]
-- columns: a: int | None (UMEDIUMINT)
-- columns: b: int | None (UMEDIUMINT(8))
-- columns: c: int | None (UINT(5))
-- columns: d: int | None (UBIGINT)
```

## Star expansion lists the columns

MySQL has no `::` cast, so `check --fix` expands `*` to the bare column names;
the schema types them.

`schema.sql`:
```sql
CREATE TABLE users (id INT NOT NULL PRIMARY KEY, name VARCHAR(20) NOT NULL);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM users
-- expanded: SELECT id, name FROM users
```

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("-- sql\nSELECT u.* FROM users u")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("-- sql\nSELECT u.id, u.name FROM users u")
```

## SUM of exact and approximate values

`SUM` of an integer or `DECIMAL` column returns `DECIMAL`, and of a `FLOAT` or
`DOUBLE` column returns `DOUBLE`, as in the
[MySQL reference](https://dev.mysql.com/doc/refman/8.4/en/aggregate-functions.html#function_sum).

`schema.sql`:
```sql
CREATE TABLE items (
    a TINYINT NOT NULL,
    b INT NOT NULL,
    c BIGINT UNSIGNED NOT NULL,
    d DECIMAL(10,2) NOT NULL,
    e FLOAT NOT NULL,
    f DOUBLE NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT SUM(a) AS a, SUM(b) AS b, SUM(c) AS c, SUM(d) AS d, SUM(e) AS e, SUM(f) AS f FROM items
-- columns: a: decimal.Decimal | None (DECIMAL)
-- columns: b: decimal.Decimal | None (DECIMAL)
-- columns: c: decimal.Decimal | None (DECIMAL)
-- columns: d: decimal.Decimal | None (DECIMAL)
-- columns: e: float | None (DOUBLE)
-- columns: f: float | None (DOUBLE)
```

## Date arithmetic

`date ± INTERVAL n unit` is a `DATE` for day units and coarser, and a
`DATETIME` otherwise. Other arithmetic on dates reads them as numbers
(`20240101`), so it is left untyped.

`schema.sql`:
```sql
CREATE TABLE events (d DATE NOT NULL, ts DATETIME NOT NULL, n INT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT d + INTERVAL 1 DAY AS a, INTERVAL 1 MONTH + d AS b, d - INTERVAL 1 HOUR AS c, ts + INTERVAL 1 DAY AS e, d + n AS f, d - d AS g FROM events
-- columns: a: datetime.date (DATE)
-- columns: b: datetime.date (DATE)
-- columns: c: datetime.datetime (DATETIME)
-- columns: e: datetime.datetime (DATETIME)
-- columns: f: None (None)
-- columns: g: None (None)
```

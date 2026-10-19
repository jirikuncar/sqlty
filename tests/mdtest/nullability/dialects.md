# Nullability: MySQL and SQLite expressions

The rules that do not depend on PostgreSQL syntax hold in every dialect; the ones
that differ follow each database.

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INT NOT NULL PRIMARY KEY,
    name VARCHAR(50) NOT NULL,
    nick VARCHAR(50),
    age INT
);
```

`IFNULL` and `COALESCE` take the first non-null argument.

```sql
SELECT CAST(ifnull(nick, name) AS CHAR), CAST(ifnull(nick, nick) AS CHAR), coalesce(age, 0) FROM users
-- inferred: tuple[str, str | None, int]
```

`CONCAT`, `GREATEST` and `LEAST` are NULL when any argument is, unlike in PostgreSQL.

```sql
SELECT CAST(concat(name, nick) AS CHAR), CAST(concat(name, name) AS CHAR), CAST(greatest(age, 1) AS SIGNED) FROM users
-- inferred: tuple[str | None, str, int | None]
```

`IF` is NULL when a branch can be.

```sql
SELECT CAST(if(age > 18, name, nick) AS CHAR), CAST(if(age > 18, name, 'minor') AS CHAR) FROM users
-- inferred: tuple[str | None, str]
```

Aggregates follow the same rules as in PostgreSQL.

```sql
SELECT count(*), max(id), max(age) FROM users
-- inferred: tuple[int, int | None, int | None]
```

```sql
SELECT name, max(id), max(age) FROM users GROUP BY name
-- inferred: tuple[str, int, int | None]
```

## SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    nick TEXT,
    age INTEGER
);
```

```sql
SELECT CAST(ifnull(nick, name) AS TEXT), coalesce(nick, name), CAST(iif(age > 18, name, nick) AS TEXT) FROM users
-- inferred: tuple[str, str, str | None]
```

The two-argument `MIN` and `MAX` are scalar functions, NULL when an argument is; with one
argument they aggregate.

```sql
SELECT max(age, 18), max(id, 1), max(id) FROM users
-- inferred: tuple[int | None, int, int | None]
```

```sql
SELECT name, max(age), max(id) FROM users GROUP BY name
-- inferred: tuple[str, int | None, int]
```

A cast keeps the nullability of what it casts. A scalar subquery with no row is NULL.

```sql
SELECT (SELECT nick FROM users u WHERE u.id = users.id + 1), (SELECT count(*) FROM users) FROM users
-- inferred: tuple[str | None, int]
```

## MySQL WITH ROLLUP nulls the grouping columns

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE sales (id INT NOT NULL PRIMARY KEY, region VARCHAR(20) NOT NULL, amount INT NOT NULL);
```

```sql
SELECT CAST(region AS CHAR), count(*), sum(amount) FROM sales GROUP BY region WITH ROLLUP
-- inferred: tuple[str | None, int, decimal.Decimal | None]
```

## Division and modulo by zero are NULL in MySQL and SQLite

PostgreSQL raises an error on a division by zero. MySQL and SQLite return NULL, so the
result is optional even for NOT NULL operands, unless the divisor is a non-zero literal.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE m (id INT NOT NULL PRIMARY KEY, x INT NOT NULL, y INT NOT NULL);
```

```sql
SELECT x / y, x % y, x / 2, x % 2, id + y FROM m
-- inferred: tuple[decimal.Decimal | None, int | None, decimal.Decimal, int, int]
```

## MySQL date arithmetic and casts to dates can be NULL

MySQL gives NULL for a date moved out of its range (`DATE '9999-12-31' + INTERVAL 1 DAY`)
and for a value that does not read as a date or time (`CAST('abc' AS DATE)`). A cast of a
value that is already a date or time cannot fail.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE e (d DATE NOT NULL, ts DATETIME NOT NULL, s VARCHAR(10) NOT NULL, n INT NOT NULL);
```

```sql
SELECT d + INTERVAL 1 DAY AS a, ts - INTERVAL 1 HOUR AS b, TIMESTAMPADD(DAY, 1, d) AS c, CAST(s AS DATE) AS f, CAST(n AS YEAR) AS g, CONVERT(s, DATETIME) AS h, CAST(ts AS DATE) AS i, CAST(d AS DATETIME) AS j FROM e
-- columns: a: datetime.date | None (DATE)
-- columns: b: datetime.datetime | None (DATETIME)
-- columns: c: datetime.date | None (DATE)
-- columns: f: datetime.date | None (DATE)
-- columns: g: int | None (YEAR)
-- columns: h: datetime.datetime | None (DATETIME)
-- columns: i: datetime.date (DATE)
-- columns: j: datetime.datetime (DATETIME)
```

## SQLite division and modulo by zero

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE m (id INTEGER NOT NULL PRIMARY KEY, x INTEGER NOT NULL, y INTEGER NOT NULL);
```

```sql
SELECT x / y, x % y, x / 2, x % 2 FROM m
-- inferred: tuple[int | None, int | None, int, int]
```

## Math functions outside their domain are NULL in MySQL and SQLite

PostgreSQL raises an error for `sqrt(-1)` or `ln(0)`. MySQL and SQLite return NULL, so
these calls are optional even for a NOT NULL argument. SQLite's `pow` is NULL where the
result is not a number, such as `pow(-8, 0.5)`.

`schema.sql`:

```sql
CREATE TABLE m (x DOUBLE NOT NULL);
```

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT SQRT(x), LN(x), LOG(x), LOG(2, x), LOG2(x), LOG10(x), ACOS(x), ASIN(x), ATAN(x), POW(x, 2) FROM m
-- inferred: tuple[float | None, float | None, float | None, float | None, float | None, float | None, float | None, float | None, float, float]
```

### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

```sql
SELECT sqrt(x), ln(x), log(x), log(2, x), log2(x), log10(x), acos(x), asin(x), atan(x), pow(x, 2) FROM m
-- inferred: tuple[float | None, float | None, float | None, float | None, float | None, float | None, float | None, float | None, float, float | None]
```

## SQLite: only an INTEGER PRIMARY KEY is never NULL

SQLite lets a PRIMARY KEY column hold NULL, unless it is declared NOT NULL, the table is
`WITHOUT ROWID` or `STRICT`, or it is the sole column of type `INTEGER`, which is the rowid.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE rowid_key (id INTEGER PRIMARY KEY);
CREATE TABLE text_key (code TEXT PRIMARY KEY);
CREATE TABLE int_key (n INT PRIMARY KEY);
CREATE TABLE declared_key (code TEXT NOT NULL PRIMARY KEY);
CREATE TABLE table_key (a INTEGER, b INTEGER, c INTEGER, PRIMARY KEY (a, b));
CREATE TABLE single_key (a INTEGER, PRIMARY KEY (a));
CREATE TABLE no_rowid (code TEXT PRIMARY KEY) WITHOUT ROWID;
CREATE TABLE strict_key (code TEXT PRIMARY KEY) STRICT;
```

```sql
SELECT id FROM rowid_key
-- inferred: tuple[int]
```

```sql
SELECT code FROM text_key
-- inferred: tuple[str | None]
```

`INT` is not the rowid alias.

```sql
SELECT n FROM int_key
-- inferred: tuple[int | None]
```

```sql
SELECT code FROM declared_key
-- inferred: tuple[str]
```

```sql
SELECT a, b, c FROM table_key
-- inferred: tuple[int | None, int | None, int | None]
```

```sql
SELECT a FROM single_key
-- inferred: tuple[int]
```

```sql
SELECT code FROM no_rowid
-- inferred: tuple[str]
```

```sql
SELECT code FROM strict_key
-- inferred: tuple[str]
```

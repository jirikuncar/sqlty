# MySQL functions

Built-in functions from the
[MySQL 8.4 reference](https://dev.mysql.com/doc/refman/8.4/en/built-in-function-reference.html),
typed as MySQL returns them.

`schema.sql`:

```sql
CREATE TABLE events (
    id INT NOT NULL PRIMARY KEY,
    name VARCHAR(50) NOT NULL,
    note VARCHAR(50),
    happened DATETIME NOT NULL,
    day DATE NOT NULL,
    price DECIMAL(10, 2) NOT NULL,
    ratio DOUBLE NOT NULL,
    doc JSON
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## Control flow

`IF` takes the common type of its two branches, `IFNULL` of its arguments.

```sql
SELECT IF(id > 1, name, note) AS a, IF(id > 1, 'big', 'small') AS b, IFNULL(note, name) AS c FROM events
-- columns: a: str | None (VARCHAR(50))
-- columns: b: typing.Literal['big', 'small'] (None)
-- columns: c: str (VARCHAR(50))
```

## Grouping

`GROUPING` tells a `WITH ROLLUP` super-aggregate row by a non-null `int`.

```sql
SELECT note, GROUPING(note) AS g FROM events GROUP BY note WITH ROLLUP
-- columns: note: str | None (VARCHAR(50))
-- columns: g: int (INT)
```

## Dates and times

```sql
SELECT NOW() AS a, CURDATE() AS b, CURTIME() AS c, DATE(happened) AS d, TIMESTAMP(day) AS e, FROM_UNIXTIME(id) AS f, LAST_DAY(day) AS g FROM events
-- columns: a: datetime.datetime (DATETIME)
-- columns: b: datetime.date (DATE)
-- columns: c: datetime.timedelta (TIME)
-- columns: d: datetime.date | None (DATE)
-- columns: e: datetime.datetime | None (DATETIME)
-- columns: f: datetime.datetime | None (DATETIME)
-- columns: g: datetime.date | None (DATE)
```

```sql
SELECT DATE_FORMAT(happened, '%Y') AS a, DAYNAME(day) AS b, YEAR(day) AS c, DATEDIFF(day, day) AS d, TIMESTAMPDIFF(DAY, day, happened) AS e FROM events
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
-- columns: c: int (BIGINT)
-- columns: d: int (BIGINT)
-- columns: e: int (BIGINT)
```

`STR_TO_DATE` returns a `DATE`, `TIME` or `DATETIME` by the parts its format
reads, and `NULL` when the string does not match.

```sql
SELECT STR_TO_DATE(name, '%Y-%m-%d') AS a, STR_TO_DATE(name, '%H:%i') AS b, STR_TO_DATE(name, '%Y-%m-%d %H:%i:%s') AS c, STR_TO_DATE(name, note) AS d FROM events
-- columns: a: datetime.date | None (DATE)
-- columns: b: datetime.timedelta | None (TIME)
-- columns: c: datetime.datetime | None (DATETIME)
-- columns: d: None (None)
```

## Strings

```sql
SELECT CONCAT_WS(',', name, note) AS a, LPAD(name, 5, '*') AS b, LEFT(name, 2) AS c, UCASE(name) AS d, MD5(name) AS e, LOCATE('a', name) AS f, FIND_IN_SET('a', name) AS g, UUID() AS h FROM events
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
-- columns: c: str (TEXT)
-- columns: d: str (TEXT)
-- columns: e: str (TEXT)
-- columns: f: int (BIGINT)
-- columns: g: int (BIGINT)
-- columns: h: str (TEXT)
```

## Numbers

```sql
SELECT ABS(price) AS a, ROUND(ratio) AS b, TRUNCATE(price, 1) AS c, FLOOR(id) AS d, MOD(id, 3) AS e, SQRT(id) AS f, POW(ratio, 2) AS g, RAND() AS h, SIGN(price) AS i FROM events
-- columns: a: decimal.Decimal (DECIMAL(10, 2))
-- columns: b: float (DOUBLE)
-- columns: c: decimal.Decimal (DECIMAL(10, 2))
-- columns: d: float (DOUBLE)
-- columns: e: int (INT)
-- columns: f: float (DOUBLE)
-- columns: g: float (DOUBLE)
-- columns: h: float (DOUBLE)
-- columns: i: int (BIGINT)
```

## JSON

```sql
SELECT JSON_OBJECT('id', id) AS a, JSON_ARRAY(id, name) AS b, JSON_EXTRACT(doc, '$.a') AS c, JSON_UNQUOTE(JSON_EXTRACT(doc, '$.a')) AS d, JSON_LENGTH(doc) AS e, JSON_VALID(doc) AS f FROM events
-- columns: a: typing.Any (JSON)
-- columns: b: typing.Any (JSON)
-- columns: c: typing.Any (JSON)
-- columns: d: str | None (TEXT)
-- columns: e: int | None (BIGINT)
-- columns: f: int | None (BIGINT)
```

```sql
SELECT JSON_ARRAYAGG(id) AS a, JSON_OBJECTAGG(name, id) AS b FROM events
-- columns: a: typing.Any (JSON)
-- columns: b: typing.Any (JSON)
```

`->` is `JSON_EXTRACT` and `->>` is `JSON_UNQUOTE(JSON_EXTRACT(...))`.

```sql
SELECT doc -> '$.a' AS a, doc ->> '$.a' AS b FROM events
-- columns: a: typing.Any (JSON)
-- columns: b: str | None (TEXT)
```

## Window functions and NULLIF

```sql
SELECT LAG(id) OVER (ORDER BY id) AS a, LEAD(name, 1, 'x') OVER (ORDER BY id) AS b, FIRST_VALUE(price) OVER (ORDER BY id) AS c, NTH_VALUE(name, 2) OVER (ORDER BY id) AS d, PERCENT_RANK() OVER (ORDER BY id) AS e, CUME_DIST() OVER (ORDER BY id) AS f, NULLIF(name, 'x') AS g FROM events
-- columns: a: int | None (INT)
-- columns: b: str (VARCHAR(50))
-- columns: c: decimal.Decimal (DECIMAL(10, 2))
-- columns: d: str | None (VARCHAR(50))
-- columns: e: float (DOUBLE)
-- columns: f: float (DOUBLE)
-- columns: g: str | None (VARCHAR(50))
```

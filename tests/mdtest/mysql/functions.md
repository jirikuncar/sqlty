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

Branches of different types meet at a common type: numbers widen, a `DATE`
and a `DATETIME` give a `DATETIME`, and a number and a string give a string.
`CASE`, `COALESCE`, `GREATEST` and `UNION` follow the same rule.

```sql
SELECT IF(id > 1, id, price) AS a, IF(id > 1, day, happened) AS b, CASE WHEN id > 1 THEN id ELSE name END AS c, COALESCE(note, price) AS d, GREATEST(id, ratio) AS e, IFNULL(doc, name) AS f FROM events
-- columns: a: decimal.Decimal (DECIMAL)
-- columns: b: datetime.datetime (DATETIME)
-- columns: c: str (TEXT)
-- columns: d: str (TEXT)
-- columns: e: float (DOUBLE)
-- columns: f: str (TEXT)
```

```sql
SELECT id AS a, day AS b FROM events UNION ALL SELECT price, happened FROM events
-- columns: a: decimal.Decimal (DECIMAL)
-- columns: b: datetime.datetime (DATETIME)
```

`ISNULL` is 1 or 0, never NULL.

```sql
SELECT ISNULL(note) AS a FROM events
-- columns: a: int (BIGINT)
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

## Date arithmetic functions

`DATE_ADD` and its kin type like `+ INTERVAL`: a date stays a date for day
units and coarser. A number of days is a day interval.

```sql
SELECT DATE_ADD(day, INTERVAL 1 DAY) AS a, DATE_SUB(day, INTERVAL 1 HOUR) AS b, ADDDATE(day, 3) AS c, SUBDATE(happened, INTERVAL 1 MONTH) AS d, ADDTIME(happened, '01:00:00') AS e FROM events
-- columns: a: datetime.date | None (DATE)
-- columns: b: datetime.datetime | None (DATETIME)
-- columns: c: datetime.date | None (DATE)
-- columns: d: datetime.datetime | None (DATETIME)
-- columns: e: datetime.datetime | None (DATETIME)
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

### Binary strings

Over a binary string, the string functions return a binary string.

`schema.sql`:

```sql
CREATE TABLE files (id INT NOT NULL PRIMARY KEY, data VARBINARY(16) NOT NULL);
```

```sql
SELECT SUBSTRING(data, 1, 2) AS a, TRIM(data) AS b, LEFT(data, 1) AS c, UPPER(data) AS d, REPLACE(data, 'a', 'b') AS e, LPAD(data, 3, 'x') AS f FROM files
-- columns: a: bytes (VARBINARY)
-- columns: b: bytes (VARBINARY)
-- columns: c: bytes (VARBINARY)
-- columns: d: bytes (VARBINARY)
-- columns: e: bytes (VARBINARY)
-- columns: f: bytes (VARBINARY)
```

Hexadecimal and bit-value literals are binary strings.

```sql
SELECT X'41' AS a, 0x41 AS b, b'101' AS c FROM files
-- columns: a: bytes (VARBINARY)
-- columns: b: bytes (VARBINARY)
-- columns: c: bytes (VARBINARY)
```

## Numbers

```sql
SELECT ABS(price) AS a, ROUND(ratio) AS b, TRUNCATE(price, 1) AS c, FLOOR(id) AS d, MOD(id, 3) AS e, SQRT(id) AS f, POW(ratio, 2) AS g, RAND() AS h, SIGN(price) AS i FROM events
-- columns: a: decimal.Decimal (DECIMAL(10, 2))
-- columns: b: float (DOUBLE)
-- columns: c: decimal.Decimal (DECIMAL(10, 2))
-- columns: d: int (BIGINT)
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

`JSON_VALUE` returns a string, or the type of its `RETURNING` clause.

```sql
SELECT JSON_VALUE(doc, '$.name') AS a, JSON_VALUE(doc, '$.age' RETURNING SIGNED) AS b FROM events
-- columns: a: str | None (TEXT)
-- columns: b: int | None (BIGINT)
```

## Rounding to an integer

`CEIL` and `FLOOR` of an exact number are integers when the integer part fits
a `BIGINT`, and of an approximate one a `DOUBLE`.

```sql
SELECT CEIL(price) AS a, FLOOR(price) AS b, CEILING(ratio) AS c, FLOOR(id) AS d FROM events
-- columns: a: int (BIGINT)
-- columns: b: int (BIGINT)
-- columns: c: float (DOUBLE)
-- columns: d: int (BIGINT)
```

## Session and conversion functions

```sql
SELECT GREATEST(id, 2) AS a, LEAST(name, note) AS b, LAST_INSERT_ID() AS c, DATABASE() AS d, VERSION() AS e, INET_ATON(name) AS f, INET_NTOA(id) AS g, UUID_TO_BIN(UUID()) AS h, BIN(id) AS i, UNIX_TIMESTAMP() AS j FROM events
-- columns: a: int (INT)
-- columns: b: str | None (VARCHAR(50))
-- columns: c: int (BIGINT)
-- columns: d: str | None (TEXT)
-- columns: e: str (TEXT)
-- columns: f: int | None (BIGINT)
-- columns: g: str | None (TEXT)
-- columns: h: bytes (VARBINARY)
-- columns: i: str (TEXT)
-- columns: j: int (BIGINT)
```

`BIT_AND`, `BIT_OR` and `BIT_XOR` are never NULL: over no rows `BIT_AND` has
every bit set and the others none.

```sql
SELECT BIT_AND(id) AS a, BIT_OR(id) AS b, STD(price) AS c FROM events
-- columns: a: int (BIGINT)
-- columns: b: int (BIGINT)
-- columns: c: float | None (DOUBLE)
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

## JSON_TABLE

`JSON_TABLE` takes its columns from its `COLUMNS` clause. A `PATH` column is NULL
where the document lacks the member; `FOR ORDINALITY` and `EXISTS PATH` never are.

```sql
SELECT jt.n, jt.x, jt.has FROM JSON_TABLE('[{"x": "a"}, {"y": 1}]', '$[*]' COLUMNS (n FOR ORDINALITY, x VARCHAR(10) PATH '$.x', has INT EXISTS PATH '$.y')) AS jt
-- columns: n: int (INT)
-- columns: x: str | None (VARCHAR(10))
-- columns: has: int (INT)
```

## CONVERT and more JSON functions

`CONVERT(x, type)` is a cast; `CONVERT(x USING charset)` keeps the string's type.

```sql
SELECT CONVERT(name, CHAR) AS a, CONVERT(id, SIGNED) AS b, CONVERT(name USING utf8mb4) AS c, ANY_VALUE(name) AS d, JSON_SEARCH(doc, 'one', 'a') AS e, JSON_PRETTY(doc) AS f FROM events
-- columns: a: str (CHAR)
-- columns: b: int (BIGINT)
-- columns: c: str (VARCHAR(50))
-- columns: d: str | None (VARCHAR(50))
-- columns: e: typing.Any (JSON)
-- columns: f: str | None (TEXT)
```

## Integer division, unsigned arithmetic and introducers

`DIV` is a `BIGINT`, NULL for a zero divisor. Integer arithmetic with an
unsigned operand is unsigned. `_binary 'a'` is a binary string.

```sql
SELECT id DIV 2 AS a, price DIV 2 AS b, CAST(id AS UNSIGNED) - 1 AS c, EXTRACT(YEAR FROM day) AS d, TIMESTAMPADD(DAY, 1, day) AS e, TIMESTAMPADD(HOUR, 1, day) AS f, _utf8mb4'a' AS g, _binary'a' AS h FROM events
-- columns: a: int (BIGINT)
-- columns: b: int (BIGINT)
-- columns: c: int (UBIGINT)
-- columns: d: int (BIGINT)
-- columns: e: datetime.date (DATE)
-- columns: f: datetime.datetime (DATETIME)
-- columns: g: str (TEXT)
-- columns: h: bytes (VARBINARY)
```

## Sets, encodings and predicates

`FROM_BASE64` is NULL for text that is not base64, and `GET_FORMAT` for an
unknown format.

```sql
SELECT EXPORT_SET(5, 'Y', 'N') AS a, MAKE_SET(1, 'a', 'b') AS b, GET_FORMAT(DATE, 'USA') AS c, FROM_BASE64(name) AS d, RANDOM_BYTES(4) AS e, COT(ratio) AS f, IS_UUID(name) AS g, IS_IPV4(name) AS h, JSON_SCHEMA_VALID('{}', doc) AS i, JSON_STORAGE_SIZE(doc) AS k, UUID_SHORT() AS l FROM events
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
-- columns: c: str | None (TEXT)
-- columns: d: bytes | None (VARBINARY)
-- columns: e: bytes (VARBINARY)
-- columns: f: float (DOUBLE)
-- columns: g: int (INT)
-- columns: h: int (INT)
-- columns: i: int | None (INT)
-- columns: k: int | None (BIGINT)
-- columns: l: int (UBIGINT)
```

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

`CONCAT`, `CONCAT_WS`, `ELT`, `EXPORT_SET` and `MAKE_SET` return a binary string when
any argument is one. A regular expression function does when its subject or pattern is
one; the replacement does not count.

```sql
SELECT CONCAT('a', data) AS a, CONCAT_WS(',', 'a', data) AS b, ELT(1, 'a', data) AS c, REGEXP_REPLACE('a', data, 'b') AS d, REGEXP_SUBSTR('a', data) AS e, REGEXP_REPLACE('a', 'b', data) AS f, CONCAT('a', 'b') AS g FROM files
-- columns: a: bytes (VARBINARY)
-- columns: b: bytes (VARBINARY)
-- columns: c: bytes | None (VARBINARY)
-- columns: d: bytes (VARBINARY)
-- columns: e: bytes | None (VARBINARY)
-- columns: f: str (TEXT)
-- columns: g: str (TEXT)
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
-- columns: f: float | None (DOUBLE)
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

`MEMBER OF` tests whether a JSON array holds a value, like `IN`:

```sql
SELECT id MEMBER OF('[1, 2]') AS a, id MEMBER OF(doc) AS b FROM events
-- columns: a: bool (BOOLEAN)
-- columns: b: bool | None (BOOLEAN)
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

`LAG` and `LEAD` return the common type of the value and the default: an `INT` with a
`DECIMAL` default is a `DECIMAL`.

```sql
SELECT LAG(id, 1, price) OVER (ORDER BY id) AS a, LEAD(id, 1, 2.5) OVER (ORDER BY id) AS b FROM events
-- columns: a: decimal.Decimal (DECIMAL)
-- columns: b: decimal.Decimal (DECIMAL)
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

`CONVERT(x, type)` is a cast.

`CONVERT(x USING charset)` and `CHAR(n, ... USING charset)` are text, or a binary string for
`USING binary`. Bytes that are not valid in the character set convert to `NULL`, so text
converted from a binary string is nullable.

```sql
SELECT CONVERT(id USING utf8mb4) AS a, CONVERT(name USING binary) AS b, CONVERT(CHAR(65) USING utf8mb4) AS c, CHAR(65, 66 USING utf8mb4) AS d, CHAR(65 USING binary) AS e FROM events
-- columns: a: str (TEXT)
-- columns: b: bytes (VARBINARY)
-- columns: c: str | None (TEXT)
-- columns: d: str | None (TEXT)
-- columns: e: bytes (VARBINARY)
```

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
-- columns: e: datetime.date | None (DATE)
-- columns: f: datetime.datetime | None (DATETIME)
-- columns: g: str (TEXT)
-- columns: h: bytes (VARBINARY)
```

`MOD` between two operands is the `%` operator, and a column keeps the name
MySQL gives it.

```sql
SELECT id MOD 3, price MOD 2 AS b, (id + 1) MOD 3 AS c, MOD(id, 0) AS d FROM events WHERE id MOD 2 = 0
-- columns: id MOD 3: int (INT)
-- columns: b: decimal.Decimal (DECIMAL)
-- columns: c: int (INT)
-- columns: d: int | None (INT)
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

## SOUNDS LIKE, INTERVAL() and LOCK IN SHARE MODE

`a SOUNDS LIKE b` compares `SOUNDEX(a) = SOUNDEX(b)`. `INTERVAL(n, n1, n2, ...)` is the
index of the first bound above `n`, and -1 when `n` is `NULL`. `LOCK IN SHARE MODE` is
the older spelling of `FOR SHARE`. A built-in function's name may be quoted.

```sql
SELECT name SOUNDS LIKE 'Robert' AS a, note SOUNDS LIKE name AS b, INTERVAL(id, 1, 10, 100) AS c, `CONCAT`(name, note) AS d FROM events LOCK IN SHARE MODE
-- columns: a: bool (BOOLEAN)
-- columns: b: bool | None (BOOLEAN)
-- columns: c: int (INT)
-- columns: d: str | None (TEXT)
-- rules: a: common.comparison / common.comparison
-- rules: b: common.comparison / common.comparison
-- rules: c: mysql.misc.interval / mysql.misc.interval
-- rules: d: mysql.string.text / mysql.string.text
```

## Bitwise operators

Bit operations on numbers give an unsigned 64-bit integer.

```sql
SELECT id & 3 AS a, id | 4 AS b, id ^ 1 AS c, ~id AS d, id << 1 AS e, id >> 1 AS f, price | 1 AS g FROM events
-- columns: a: int (UBIGINT)
-- columns: b: int (UBIGINT)
-- columns: c: int (UBIGINT)
-- columns: d: int (UBIGINT)
-- columns: e: int (UBIGINT)
-- columns: f: int (UBIGINT)
-- columns: g: int (UBIGINT)
```

## Strings and JSON, more functions

`CHAR` skips `NULL` codes, so it is never `NULL`. `LOAD_FILE` is `NULL` for a
file the server cannot read.

```sql
SELECT CHAR(id) AS a, CHARACTER_LENGTH(name) AS b, MID(name, 1, 2) AS c, LOAD_FILE(name) AS d, JSON_MERGE(doc, doc) AS e, JSON_SCHEMA_VALIDATION_REPORT('{}', doc) AS f FROM events
-- columns: a: bytes (VARBINARY)
-- columns: b: int (INT)
-- columns: c: str (TEXT)
-- columns: d: bytes | None (VARBINARY)
-- columns: e: typing.Any (JSON)
-- columns: f: typing.Any (JSON)
-- rules: a: mysql.string.char / mysql.string.char
-- rules: b: mysql.string.character-length / mysql.string.character-length
-- rules: c: mysql.string.string / mysql.string.string
-- rules: d: mysql.string.load-file / mysql.string.load-file
-- rules: e: mysql.json.merge / mysql.json.merge
-- rules: f: mysql.json.validation-report / mysql.json.validation-report
```

## Locks, replication and digests

```sql
SELECT GET_LOCK(name, 0) AS a, IS_USED_LOCK(name) AS b, RELEASE_ALL_LOCKS() AS c, PS_CURRENT_THREAD_ID() AS d, FORMAT_BYTES(id) AS e, GTID_SUBSET(name, name) AS f, GTID_SUBTRACT(name, name) AS g FROM events
-- columns: a: int | None (INT)
-- columns: b: int | None (UBIGINT)
-- columns: c: int (UBIGINT)
-- columns: d: int | None (UBIGINT)
-- columns: e: str (TEXT)
-- columns: f: int (BIGINT)
-- columns: g: str (TEXT)
-- rules: a: mysql.system.lock / mysql.system.lock
-- rules: b: mysql.system.lock-owner / mysql.system.lock-owner
-- rules: c: mysql.system.release-all-locks / mysql.system.release-all-locks
-- rules: d: mysql.system.thread-id / mysql.system.thread-id
-- rules: e: mysql.system.format / mysql.system.format
-- rules: f: mysql.system.gtid-subset / mysql.system.gtid-subset
-- rules: g: mysql.system.gtid-subtract / mysql.system.gtid-subtract
```

```sql
SELECT SOURCE_POS_WAIT(name, id) AS a, ROLES_GRAPHML() AS b, STATEMENT_DIGEST(name) AS c, UNCOMPRESSED_LENGTH(name) AS d, VALIDATE_PASSWORD_STRENGTH(name) AS e FROM events
-- columns: a: int | None (BIGINT)
-- columns: b: str | None (TEXT)
-- columns: c: str (TEXT)
-- columns: d: int (INT)
-- columns: e: int (INT)
-- rules: a: mysql.system.source-pos-wait / mysql.system.source-pos-wait
-- rules: b: mysql.system.roles-graphml / mysql.system.roles-graphml
-- rules: c: mysql.encryption.digest / mysql.encryption.digest
-- rules: d: mysql.encryption.int / mysql.encryption.int
-- rules: e: mysql.encryption.int / mysql.encryption.int
```

## Spatial functions

A geometry is returned as bytes, in MySQL's internal format: a 4-byte SRID
followed by the WKB. A relation between geometries is `NULL` when one of them
is empty, and so is a part of a geometry that has no such part.

`schema.sql`:

```sql
CREATE TABLE places (
    id INT NOT NULL PRIMARY KEY,
    spot POINT NOT NULL,
    area POLYGON NOT NULL,
    route LINESTRING NOT NULL,
    shape GEOMETRY
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT ST_Contains(area, spot) AS a, MBRWithin(spot, area) AS b, ST_Distance(spot, area) AS c, ST_X(spot) AS d, ST_X(spot, 2) AS e, ST_NumPoints(route) AS f, ST_IsEmpty(shape) AS g FROM places
-- columns: a: int | None (INT)
-- columns: b: int | None (INT)
-- columns: c: float | None (DOUBLE)
-- columns: d: float (DOUBLE)
-- columns: e: bytes (GEOMETRY)
-- columns: f: int | None (INT)
-- columns: g: int | None (INT)
-- rules: a: mysql.spatial.relation / mysql.spatial.relation
-- rules: b: mysql.spatial.relation / mysql.spatial.relation
-- rules: c: mysql.spatial.double-nullable / mysql.spatial.double-nullable
-- rules: d: mysql.spatial.double / mysql.spatial.double
-- rules: e: mysql.spatial.set-coordinate / mysql.spatial.set-coordinate
-- rules: f: mysql.spatial.int-nullable / mysql.spatial.int-nullable
-- rules: g: mysql.spatial.int / mysql.spatial.int
```

```sql
SELECT Point(1, 2) AS a, ST_GeomFromText('POINT(1 1)') AS b, ST_Centroid(area) AS c, ST_AsText(spot) AS d, ST_AsBinary(spot) AS e, ST_AsGeoJSON(spot) AS f, ST_LatFromGeoHash('s0') AS g FROM places
-- columns: a: bytes (POINT)
-- columns: b: bytes (GEOMETRY)
-- columns: c: bytes | None (GEOMETRY)
-- columns: d: str (TEXT)
-- columns: e: bytes (VARBINARY)
-- columns: f: typing.Any (JSON)
-- columns: g: float (DOUBLE)
-- rules: a: mysql.spatial.point / mysql.spatial.point
-- rules: b: mysql.spatial.geometry / mysql.spatial.geometry
-- rules: c: mysql.spatial.geometry-nullable / mysql.spatial.geometry-nullable
-- rules: d: mysql.spatial.text / mysql.spatial.text
-- rules: e: mysql.spatial.varbinary / mysql.spatial.varbinary
-- rules: f: mysql.spatial.geojson / mysql.spatial.geojson
-- rules: g: mysql.spatial.double / mysql.spatial.double
```

```sql
SELECT ST_Collect(spot) AS a FROM places
-- columns: a: bytes | None (GEOMETRY)
-- rules: a: mysql.spatial.collect / mysql.spatial.collect
```

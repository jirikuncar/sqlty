# PostgreSQL built-in functions and operators

Regular expressions, ordered-set and statistics aggregates, array, network and
`jsonb` functions and operators, typed as PostgreSQL returns them.

`schema.sql`:

```sql
CREATE TABLE t (
    id INTEGER PRIMARY KEY,
    i INTEGER NOT NULL,
    b BIGINT,
    d DOUBLE PRECISION NOT NULL,
    n NUMERIC(10, 2),
    tx TEXT NOT NULL,
    ts TIMESTAMP NOT NULL,
    tz TIMESTAMPTZ NOT NULL,
    dt DATE NOT NULL,
    iv INTERVAL NOT NULL,
    jb JSONB NOT NULL,
    by BYTEA NOT NULL,
    ia INTEGER[] NOT NULL,
    ip INET NOT NULL,
    cd CIDR NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Regular expressions and time zones

`regexp_match` is NULL when nothing matches. `timezone(zone, x)` is
`x AT TIME ZONE zone`.

```sql
SELECT regexp_match(tx, 'a') AS m, regexp_matches(tx, 'a') AS ms, date_bin('15 minutes', ts, ts) AS bin, timezone('UTC', tz) AS local, timezone('UTC', ts) AS utc FROM t
-- columns: m: list[str] | None (ARRAY<TEXT>)
-- columns: ms: list[str] (ARRAY<TEXT>)
-- columns: bin: datetime.datetime (TIMESTAMP)
-- columns: local: datetime.datetime (TIMESTAMP)
-- columns: utc: datetime.datetime (TIMESTAMPTZ)
```

## Function-style casts

`date(x)` is `x::date`, written as a call.

```sql
SELECT date(ts) AS day, date(tz) AS local FROM t
-- columns: day: datetime.date (DATE)
-- columns: local: datetime.date (DATE)
```

```toml
[tool.sqlty.rules]
unknown-function = "error"
```

```python
import datetime

from sqlty import SQL

# error:
q = SQL[tuple[datetime.date]]("--sql\nSELECT date(ts) FROM t")
```

## Statistics and ordered-set aggregates

`percentile_cont` interpolates, so it is a `double precision` (an `interval` over
intervals); `percentile_disc` and `mode` pick a value of the ordered column.
An array of fractions gives an array.

```sql
SELECT bit_and(i) AS a, bit_or(b) AS o, corr(d, d) AS c, covar_pop(i, i) AS cp, regr_slope(d, i) AS rs, regr_count(d, i) AS rc FROM t
-- columns: a: int | None (INT)
-- columns: o: int | None (BIGINT)
-- columns: c: float | None (DOUBLE)
-- columns: cp: float | None (DOUBLE)
-- columns: rs: float | None (DOUBLE)
-- columns: rc: int (BIGINT)
```

```sql
SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY i) AS p, percentile_cont(0.5) WITHIN GROUP (ORDER BY iv) AS pi, percentile_disc(0.5) WITHIN GROUP (ORDER BY tx) AS pd, mode() WITHIN GROUP (ORDER BY dt) AS mo, percentile_cont(ARRAY[0.5, 0.9]) WITHIN GROUP (ORDER BY n) AS ps FROM t
-- columns: p: float | None (DOUBLE)
-- columns: pi: datetime.timedelta | None (INTERVAL)
-- columns: pd: str | None (TEXT)
-- columns: mo: datetime.date | None (DATE)
-- columns: ps: list[float] | None (ARRAY<DOUBLE>)
```

```sql
SELECT percent_rank() OVER (ORDER BY i) AS pr, cume_dist() OVER (ORDER BY i) AS cd FROM t
-- columns: pr: float (DOUBLE)
-- columns: cd: float (DOUBLE)
```

## Arrays and bytes

`array_dims`, `array_lower` and `array_upper` are NULL for an empty array.

```sql
SELECT array_positions(ia, 1) AS ps, array_dims(ia) AS dims, array_lower(ia, 1) AS lo, array_upper(ia, 1) AS hi, array_fill(1, ARRAY[2]) AS f, get_byte(by, 0) AS gb FROM t
-- columns: ps: list[int] (ARRAY<INT>)
-- columns: dims: str | None (TEXT)
-- columns: lo: int | None (INT)
-- columns: hi: int | None (INT)
-- columns: f: list[int] (ARRAY<INT>)
-- columns: gb: int (INT)
```

`substring`, `substr`, `overlay` and the trim functions have `bytea`
overloads, which return `bytea`.

```sql
SELECT substring(by from 2 for 3) AS a, substr(by, 2) AS b, overlay(by placing '\x00'::bytea from 1) AS c, btrim(by, '\x00'::bytea) AS d, trim(both '\x00'::bytea from by) AS e, substring(tx from 2) AS f FROM t
-- columns: a: bytes (VARBINARY)
-- columns: b: bytes (VARBINARY)
-- columns: c: bytes (VARBINARY)
-- columns: d: bytes (VARBINARY)
-- columns: e: bytes (VARBINARY)
-- columns: f: str (TEXT)
```

## Trigonometry in degrees and hyperbolic functions

```sql
SELECT cot(d) AS a, sind(d) AS b, atan2d(d, d) AS c, tanh(d) AS e, atanh(d) AS f FROM t
-- columns: a: float (DOUBLE)
-- columns: b: float (DOUBLE)
-- columns: c: float (DOUBLE)
-- columns: e: float (DOUBLE)
-- columns: f: float (DOUBLE)
```

## Positions, normalization and identifiers

`IS NORMALIZED` of NULL is NULL.

```sql
SELECT position('a' in tx) AS a, tx IS NFC NORMALIZED AS b, n::text IS NOT NFD NORMALIZED AS c, parse_ident(tx) AS e, pg_postmaster_start_time() AS f FROM t
-- columns: a: int (INT)
-- columns: b: bool (BOOLEAN)
-- columns: c: bool | None (BOOLEAN)
-- columns: e: list[str] (ARRAY<TEXT>)
-- columns: f: datetime.datetime (TIMESTAMPTZ)
```

## Network addresses

`<<`, `<<=`, `>>` and `>>=` test containment. An address plus or minus an
integer is an address, and the difference of two addresses is a `bigint`.
`inet_client_addr()` is NULL over a Unix socket.

```sql
SELECT host(ip) AS h, abbrev(cd) AS ab, masklen(cd) AS ml, family(ip) AS fam, network(ip) AS net, inet_merge(ip, ip) AS merged, broadcast(ip) AS bc, set_masklen(cd, 8) AS sm, inet_client_addr() AS client FROM t
-- columns: h: str (TEXT)
-- columns: ab: str (TEXT)
-- columns: ml: int (INT)
-- columns: fam: int (INT)
-- columns: net: ipaddress.IPv4Network | ipaddress.IPv6Network (CIDR)
-- columns: merged: ipaddress.IPv4Network | ipaddress.IPv6Network (CIDR)
-- columns: bc: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: sm: ipaddress.IPv4Network | ipaddress.IPv6Network (CIDR)
-- columns: client: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface | None (INET)
```

```sql
SELECT ip << cd AS a, ip <<= cd AS b, cd >> ip AS c, cd >>= ip AS d, ip & ip AS e, ip + 1 AS f, ip - 1 AS g, ip - ip AS h FROM t
-- columns: a: bool (BOOLEAN)
-- columns: b: bool (BOOLEAN)
-- columns: c: bool (BOOLEAN)
-- columns: d: bool (BOOLEAN)
-- columns: e: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: f: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: g: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: h: int (BIGINT)
```

## jsonb minus

```sql
SELECT jb - 'a' AS a, jb - ARRAY['a', 'b'] AS b FROM t
-- columns: a: typing.Any (JSONB)
-- columns: b: typing.Any (JSONB)
```

## More built-ins

Typed as PostgreSQL 17's `pg_typeof` reports them. A set-returning function in the select
list gives one value per row.

```sql
SELECT timeofday() AS a, pg_column_size(tx) AS b, regexp_count(tx, 'a') AS c, scale(n) AS d, log10(n) AS e, log10(d) AS f, factorial(5) AS g, jsonb_exists(jb, 'a') AS h, has_table_privilege('t', 'select') AS i, format_type(23, NULL) AS j, pg_get_userbyid(10) AS k, unistr(tx) AS l FROM t
-- columns: a: str (TEXT)
-- columns: b: int (INT)
-- columns: c: int (INT)
-- columns: d: int | None (INT)
-- columns: e: decimal.Decimal | None (DECIMAL)
-- columns: f: float (DOUBLE)
-- columns: g: decimal.Decimal (DECIMAL)
-- columns: h: bool (BOOLEAN)
-- columns: i: bool (BOOLEAN)
-- columns: j: str | None (TEXT)
-- columns: k: str (NAME)
-- columns: l: str (TEXT)
```

```sql
SELECT jsonb_object_keys(jb) AS k, regexp_split_to_table(tx, ',') AS part, jsonb_each(jb) AS kv FROM t
-- columns: k: str | None (TEXT)
-- columns: part: str | None (TEXT)
-- columns: kv: tuple[typing.Any, ...] | None (RECORD)
```

## Scales, error functions, JSON paths and range bounds

`jsonb_path_match` is NULL when the path's first result is not a boolean.

```sql
SELECT min_scale(n) AS a, trim_scale(n) AS b, erf(d) AS c, regexp_instr(tx, 'a') AS e, regexp_substr(tx, 'a') AS f, jsonb_path_match(jb, '$.a == 1') AS g, jsonb_path_query_array(jb, '$.a') AS h, set_byte(by, 0, 1) AS i, bit_count(by) AS k, lower_inc(int4range(1, 5)) AS l, range_merge(int4range(1, 2), int4range(3, 4)) AS m FROM t
-- columns: a: int | None (INT)
-- columns: b: decimal.Decimal | None (DECIMAL)
-- columns: c: float (DOUBLE)
-- columns: e: int (INT)
-- columns: f: str | None (TEXT)
-- columns: g: bool | None (BOOLEAN)
-- columns: h: typing.Any (JSONB)
-- columns: i: bytes (VARBINARY)
-- columns: k: int (BIGINT)
-- columns: l: bool (BOOLEAN)
-- columns: m: None (INT4RANGE)
```

```sql
SELECT range_intersect_agg(int4range(i, i + 1)) AS r FROM t
-- columns: r: None (INT4RANGE)
```

## Operators on bytes, roots, jsonb subscripts and ranges

A `jsonb` subscript is `jsonb`, NULL for a missing key. `|/` and `||/` take a
`double precision`.

```sql
SELECT by || by AS a, |/ d AS b, ||/ i AS c, jb['a'] AS e, jb['a']['b'] AS f, int4range(1, 5) * int4range(3, 7) AS g, int4range(1, 5) + int4range(3, 7) AS h, int4range(1, 5) - int4range(3, 7) AS k FROM t
-- columns: a: bytes (VARBINARY)
-- columns: b: float (DOUBLE)
-- columns: c: float (DOUBLE)
-- columns: e: typing.Any (JSONB)
-- columns: f: typing.Any (JSONB)
-- columns: g: None (INT4RANGE)
-- columns: h: None (INT4RANGE)
-- columns: k: None (INT4RANGE)
```

## Examples of the documentation

`scripts/compat/pg_docs.py` compares the examples of the PostgreSQL documentation with
the server. These are the ones it found typed differently. `pg_typeof` returns a
`regtype`, `cbrt` only has a `double precision` overload, `length` of a path is a
distance, and `&&`, `||` and `<->` combine text search queries. `substr` and
`IS NORMALIZED` name their columns after the functions they call.

```sql
SELECT pg_typeof(33) AS typ, cbrt(64.0) AS root, length(path '((-1,0),(1,0))') AS len,
    'fat | rat'::tsquery && 'cat'::tsquery AS q,
    ts_headline('{"cat":"raining cats and dogs"}'::jsonb, 'cat') AS headline
-- columns: typ: str (REGTYPE)
-- columns: root: float (DOUBLE)
-- columns: len: float (DOUBLE)
-- columns: q: str (TSQUERY)
-- columns: headline: typing.Any (JSONB)
```

```sql
SELECT substr('alphabet', 3), U&'\0061\0308bc' IS NFD NORMALIZED, 'a' IS NOT NORMALIZED
-- columns: substr: str (TEXT)
-- columns: is_normalized: bool (BOOLEAN)
-- columns: ?column?: bool (BOOLEAN)
```

## Signatures of the server

`scripts/compat/pg_docs.py` also calls every overload of the documented functions and every
operator. `ts_headline` keeps the JSON type of its document after a configuration,
`string_agg` of `bytea` is `bytea`, and `age` of a transaction ID counts transactions. The
session functions return `name`. PostgreSQL has one array type per element type, so
`array_agg` of arrays, the `ARRAY` of arrays and `unnest` of a two-dimensional array follow
the dimensions, and `unnest` returns the elements.

```sql
SELECT ts_headline('english', '{"a": "cats"}'::jsonb, 'cat') AS headline,
    string_agg('\x01'::bytea, '\x00'::bytea) AS bytes, age('5'::xid) AS xid_age,
    current_database() AS db, current_user AS usr, array_agg(ARRAY[1, 2]) AS grid,
    ARRAY[ARRAY[1, 2], ARRAY[3, 4]] AS matrix, unnest(ARRAY[[1, 2], [3, 4]]) AS element
-- columns: headline: typing.Any (JSONB)
-- columns: bytes: bytes | None (BYTEA)
-- columns: xid_age: int (INT)
-- columns: db: str (NAME)
-- columns: usr: str (NAME)
-- columns: grid: list[list[int]] | None (ARRAY<ARRAY<INT>>)
-- columns: matrix: list[list[int]] (ARRAY<ARRAY<INT>>)
-- columns: element: int | None (INT)
```

## Geometry, XML, system information and multiranges

Every function the documentation lists has a type: the geometric functions return their
geometric type, the XML functions `xml` or `boolean`, and the WAL and snapshot functions
`pg_lsn` and `pg_snapshot`, which psycopg loads as `str`. The catalog lookups `to_regclass`
and friends are NULL for a name that does not exist. A multirange keeps its range type:
`range_merge` and `unnest` of a multirange give the range type, `multirange` and
`range_agg` the multirange type, and `lower` the bound type.

```sql
SELECT area(box '(2,2),(0,0)') AS area, center(box '(1,2),(0,0)') AS center,
    xpath('/a', '<a/>'::xml) AS nodes, xml_is_well_formed('<a/>') AS well_formed,
    pg_current_wal_lsn() AS lsn, pg_current_snapshot() AS snapshot,
    to_regclass('users') AS relation, gcd(1071, 462) AS divisor,
    range_merge('{[1,2), [3,4)}'::int4multirange) AS merged,
    multirange(int4range(1, 2)) AS multi, lower('{[1.1,2.2)}'::nummultirange) AS low,
    timezone(timestamptz '2001-02-16 20:38:40-05') AS local_time
-- columns: area: float (DOUBLE)
-- columns: center: str (POINT)
-- columns: nodes: list[str] (ARRAY<XML>)
-- columns: well_formed: bool (BOOLEAN)
-- columns: lsn: str | None (PG_LSN)
-- columns: snapshot: str (PG_SNAPSHOT)
-- columns: relation: str | None (REGCLASS)
-- columns: divisor: int (INT)
-- columns: merged: None (INT4RANGE)
-- columns: multi: None (INT4MULTIRANGE)
-- columns: low: decimal.Decimal | None (DECIMAL)
-- columns: local_time: datetime.datetime (TIMESTAMP)
```

## Operators on shapes, money, log locations and bit strings

Every operator of `pg_operator` has a type. Shape operators give a shape, a point, a
distance or a predicate. `#` and `##` are NULL when the shapes do not meet. An amount
divided by an amount is a `double precision`, and the distance between two log locations
is a `numeric`.

```sql
SELECT box '(2,2),(0,0)' + point '(1,1)' AS moved, point '(0,0)' <-> point '(3,4)' AS distance,
    lseg '[(0,0),(1,1)]' # lseg '[(0,1),(1,0)]' AS crossing, @@ circle '<(1,1),2>' AS center,
    # path '[(0,0),(1,1)]' AS points, 10::money / 4::money AS ratio,
    '1/0'::pg_lsn - '0/1'::pg_lsn AS distance_bytes, B'1010' # B'0110' AS xor,
    B'10' || B'01' AS bits
-- columns: moved: str (BOX)
-- columns: distance: float (DOUBLE)
-- columns: crossing: str | None (POINT)
-- columns: center: str (POINT)
-- columns: points: int (INT)
-- columns: ratio: float (DOUBLE)
-- columns: distance_bytes: decimal.Decimal (DECIMAL)
-- columns: xor: str (BIT)
-- columns: bits: str (VARBIT)
```

## Syntax sqlparser does not read

sqlparser rejects some PostgreSQL syntax. When a query does not parse, sqlty respells that
syntax as an equivalent it does read, so the query is typed as PostgreSQL types it:

| Syntax | Read as |
|---|---|
| `x BETWEEN SYMMETRIC a AND b` | `x BETWEEN a AND b` |
| `ts AT LOCAL` | `ts AT TIME ZONE current_setting('TimeZone')` |
| `COLLATION FOR (x)` | `pg_collation_for(x)` |
| `trim(both from x, chars)` | `btrim(x, chars)` |
| `substring(x similar p escape e)` | `substring(x from p for e)` |
| `JSON_VALUE(x, path PASSING ... DEFAULT ... ON ERROR)` | `JSON_VALUE(x, path)`, keeping `RETURNING` |
| `json_array(SELECT ...)` | `json_array((SELECT ...))` |

```sql
SELECT 2 BETWEEN SYMMETRIC 3 AND 1 AS swapped, timestamp '2001-02-16 20:38:40' at local AS local, collation for ('foo'::text) AS collation, trim(both from 'yxTomxx', 'xyz') AS trimmed, substring('Thomas' similar '%#"o_a#"_' escape '#') AS matched, JSON_VALUE(jsonb '[1,2]', 'strict $[$off]' PASSING 1 AS off RETURNING int DEFAULT 9 ON ERROR) AS value, json_array(SELECT 1) AS arr
-- columns: swapped: bool (BOOLEAN)
-- columns: local: datetime.datetime (TIMESTAMPTZ)
-- columns: collation: str | None (TEXT)
-- columns: trimmed: str (TEXT)
-- columns: matched: str | None (TEXT)
-- columns: value: int | None (INT)
-- columns: arr: typing.Any (JSON)
```

```sql
SELECT 2 NOT BETWEEN ASYMMETRIC 1 AND 3 AS outside, trim(leading from 'xxa', 'x') AS lead, trim(trailing from 'axx', 'x') AS trail, JSON_QUERY(jsonb '[1,[2,3],null]', 'lax $[*][$off]' PASSING 1 AS off WITH CONDITIONAL WRAPPER) AS wrapped, JSON_QUERY(jsonb '{"a": "[1, 2]"}', 'lax $.a' RETURNING int[] OMIT QUOTES ERROR ON ERROR) AS unquoted, JSON_EXISTS(jsonb '{"a": [1,2,3]}', 'lax $.a[5]' ERROR ON ERROR) AS found, 'at local' AS text -- at local
-- columns: outside: bool (BOOLEAN)
-- columns: lead: str (TEXT)
-- columns: trail: str (TEXT)
-- columns: wrapped: typing.Any (JSONB)
-- columns: unquoted: list[int] | None (ARRAY<INT>)
-- columns: found: bool (BOOLEAN)
-- columns: text: typing.Literal['at local'] (None)
```

`UNKNOWN ON ERROR` makes `JSON_EXISTS` NULL on an error, so sqlty leaves that call to sqlparser:

```python
# error: [unparsed-query] "query does not parse: Expected: ), found: UNKNOWN at Line: 1, Column: 47"
q = "--sql\nSELECT JSON_EXISTS(jsonb '[1]', 'strict $[5]' UNKNOWN ON ERROR)"
```

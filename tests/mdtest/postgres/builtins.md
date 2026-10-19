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

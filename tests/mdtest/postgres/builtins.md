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

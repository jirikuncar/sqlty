# Date, time, interval and address arithmetic

PostgreSQL's date/time operators (Table 9.32) and address operators (Table
9.39), from the rules in `rules/postgres/arithmetic.inference.toml`. A result
is NULL when an operand is.

`schema.sql`:

```sql
CREATE TABLE t (
    d DATE NOT NULL,
    t TIME NOT NULL,
    tz TIMETZ NOT NULL,
    ts TIMESTAMP NOT NULL,
    tstz TIMESTAMPTZ NOT NULL,
    iv INTERVAL NOT NULL,
    n INTEGER NOT NULL,
    s SMALLINT NOT NULL,
    x NUMERIC(10, 2) NOT NULL,
    a INET NOT NULL,
    c CIDR NOT NULL,
    maybe_d DATE,
    maybe_iv INTERVAL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Moving by an interval

A date becomes a timestamp; timestamps and times keep their type.

```sql
SELECT d + iv AS a, d - iv AS b, iv + d AS c, ts + iv AS e, tstz - iv AS f, iv + tstz AS g, t + iv AS h, tz - iv AS i, iv + t AS j FROM t
-- columns: a: datetime.datetime (TIMESTAMP)
-- columns: b: datetime.datetime (TIMESTAMP)
-- columns: c: datetime.datetime (TIMESTAMP)
-- columns: e: datetime.datetime (TIMESTAMP)
-- columns: f: datetime.datetime (TIMESTAMPTZ)
-- columns: g: datetime.datetime (TIMESTAMPTZ)
-- columns: h: datetime.time (TIME)
-- columns: i: datetime.time (TIMETZ)
-- columns: j: datetime.time (TIME)
```

## Days, dates and times

```sql
SELECT d + n AS a, d - s AS b, n + d AS c, d - d AS e, d + t AS f, t + d AS g, d + tz AS h, tz + d AS i FROM t
-- columns: a: datetime.date (DATE)
-- columns: b: datetime.date (DATE)
-- columns: c: datetime.date (DATE)
-- columns: e: int (INT)
-- columns: f: datetime.datetime (TIMESTAMP)
-- columns: g: datetime.datetime (TIMESTAMP)
-- columns: h: datetime.datetime (TIMESTAMPTZ)
-- columns: i: datetime.datetime (TIMESTAMPTZ)
```

## Differences

```sql
SELECT t - t AS a, ts - ts AS b, tstz - tstz AS c FROM t
-- columns: a: datetime.timedelta (INTERVAL)
-- columns: b: datetime.timedelta (INTERVAL)
-- columns: c: datetime.timedelta (INTERVAL)
```

## Intervals

```sql
SELECT iv + iv AS a, iv - iv AS b, iv * n AS c, x * iv AS e, iv / x AS f FROM t
-- columns: a: datetime.timedelta (INTERVAL)
-- columns: b: datetime.timedelta (INTERVAL)
-- columns: c: datetime.timedelta (INTERVAL)
-- columns: e: datetime.timedelta (INTERVAL)
-- columns: f: datetime.timedelta (INTERVAL)
```

## Nullable operands

```sql
SELECT maybe_d + 1 AS a, d + maybe_iv AS b, maybe_iv / 2 AS c FROM t
-- columns: a: datetime.date | None (DATE)
-- columns: b: datetime.datetime | None (TIMESTAMP)
-- columns: c: datetime.timedelta | None (INTERVAL)
```

## Addresses

```sql
SELECT a - c AS a, a + n AS b, c - n AS c, n + a AS e FROM t
-- columns: a: int (BIGINT)
-- columns: b: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: c: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: e: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
```

## Exponentiation

`numeric ^ numeric` covers an integer next to a numeric; anything else is
`double precision`.

```sql
SELECT x ^ n AS a, n ^ x AS b, x ^ x AS c, n ^ n AS e FROM t
-- columns: a: decimal.Decimal (DECIMAL)
-- columns: b: decimal.Decimal (DECIMAL)
-- columns: c: decimal.Decimal (DECIMAL)
-- columns: e: float (DOUBLE)
```

# PostgreSQL date and time arithmetic

Operators on `date`, `time`, `timestamp`, `timestamptz` and `interval` follow the
[PostgreSQL date/time operators table](https://www.postgresql.org/docs/current/functions-datetime.html#OPERATORS-DATETIME-TABLE).

`schema.sql`:

```sql
CREATE TABLE t (
    d DATE NOT NULL,
    tm TIME NOT NULL,
    ttz TIMETZ NOT NULL,
    ts TIMESTAMP NOT NULL,
    tz TIMESTAMPTZ NOT NULL,
    iv INTERVAL NOT NULL,
    i INTEGER NOT NULL,
    f DOUBLE PRECISION NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Addition

```sql
SELECT d + i AS a, i + d AS b, d + iv AS c, iv + d AS e, d + tm AS g, tm + d AS h, d + ttz AS j, iv + iv AS k, ts + iv AS l, iv + ts AS m, tz + iv AS n, tm + iv AS o, iv + tm AS p, ttz + iv AS q FROM t
-- columns: a: datetime.date (DATE)
-- columns: b: datetime.date (DATE)
-- columns: c: datetime.datetime (TIMESTAMP)
-- columns: e: datetime.datetime (TIMESTAMP)
-- columns: g: datetime.datetime (TIMESTAMP)
-- columns: h: datetime.datetime (TIMESTAMP)
-- columns: j: datetime.datetime (TIMESTAMPTZ)
-- columns: k: datetime.timedelta (INTERVAL)
-- columns: l: datetime.datetime (TIMESTAMP)
-- columns: m: datetime.datetime (TIMESTAMP)
-- columns: n: datetime.datetime (TIMESTAMPTZ)
-- columns: o: datetime.time (TIME)
-- columns: p: datetime.time (TIME)
-- columns: q: datetime.time (TIMETZ)
```

## Subtraction

```sql
SELECT d - d AS a, d - i AS b, d - iv AS c, tm - tm AS e, tm - iv AS g, ttz - iv AS h, ts - iv AS j, tz - iv AS k, iv - iv AS l, ts - ts AS m, tz - tz AS n FROM t
-- columns: a: int (INT)
-- columns: b: datetime.date (DATE)
-- columns: c: datetime.datetime (TIMESTAMP)
-- columns: e: datetime.timedelta (INTERVAL)
-- columns: g: datetime.time (TIME)
-- columns: h: datetime.time (TIMETZ)
-- columns: j: datetime.datetime (TIMESTAMP)
-- columns: k: datetime.datetime (TIMESTAMPTZ)
-- columns: l: datetime.timedelta (INTERVAL)
-- columns: m: datetime.timedelta (INTERVAL)
-- columns: n: datetime.timedelta (INTERVAL)
```

## Negation, multiplication and division

```sql
SELECT -iv AS a, iv * f AS b, f * iv AS c, iv * i AS e, iv / f AS g FROM t
-- columns: a: datetime.timedelta (INTERVAL)
-- columns: b: datetime.timedelta (INTERVAL)
-- columns: c: datetime.timedelta (INTERVAL)
-- columns: e: datetime.timedelta (INTERVAL)
-- columns: g: datetime.timedelta (INTERVAL)
```

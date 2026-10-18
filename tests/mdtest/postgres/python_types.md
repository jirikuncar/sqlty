# Python types psycopg returns

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## NUMERIC is Decimal

```sql
SELECT total FROM orders
-- inferred: tuple[decimal.Decimal]
```

## Fractional literals and AVG are Decimal

```sql
SELECT 1.5, AVG(total), AVG(total::DOUBLE PRECISION) FROM orders
-- inferred: tuple[decimal.Decimal, decimal.Decimal | None, float | None]
```

## JSONB holds any JSON value

```sql
SELECT '[]'::jsonb AS tags
-- inferred: tuple[typing.Any]
```

## JSON functions

```sql
SELECT to_jsonb(id), jsonb_agg(id), jsonb_build_object('a', id) FROM users
-- inferred: tuple[typing.Any, typing.Any, typing.Any]
```

## Arrays keep their element type

```sql
SELECT ARRAY[name]::text[] AS names FROM users
-- inferred: tuple[list[str]]
```

```sql
SELECT '{}'::numeric[], '{}'::int[][], '{}'::jsonb[]
-- inferred: tuple[list[decimal.Decimal], list[list[int]], list[typing.Any]]
```

## inet and cidr

```sql
SELECT '1.2.3.4'::inet, '1.2.3.0/24'::cidr
-- inferred: tuple[ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface, ipaddress.IPv4Network | ipaddress.IPv6Network]
```

A literal prefixed with its type name, like `INET '1.2.3.4'`, types like the
cast and is named after the type.

```sql
SELECT INET '10.0.0.1', CIDR '10.0.0.0/8', MACADDR '08:00:2b:01:02:03'
-- columns: inet: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- columns: cidr: ipaddress.IPv4Network | ipaddress.IPv6Network (CIDR)
-- columns: macaddr: str (MACADDR)
```

`macaddr8` is `str` too.

```sql
SELECT '08:00:2b:01:02:03:04:05'::macaddr8 AS m
-- columns: m: str (MACADDR8)
```

## Records are tuples

A row constructor, `ROW(...)` or `(a, b)`, is an anonymous `record`. psycopg
loads it as a tuple, of strings in the default text format, so its fields
are `Any`.

```sql
SELECT ROW(1, 'a'), (1, 'a') AS pair
-- columns: row: tuple[typing.Any, ...] (RECORD)
-- columns: pair: tuple[typing.Any, ...] (RECORD)
```

## Text-like types are str

```sql
SELECT 'a'::citext, 'a'::name, 'a'::xml, 'a'::tsvector, 'a'::money, 'a'::macaddr
-- inferred: tuple[str, str, str, str, str, str]
```

The same types as prefixed literals:

```sql
SELECT citext 'a', name 'a', money '1', bpchar 'a'
-- columns: citext: str (CITEXT)
-- columns: name: str (NAME)
-- columns: money: str (MONEY)
-- columns: bpchar: str (BPCHAR)
```

`X'41'` and `B'101'` are bit strings.

```sql
SELECT X'41' AS a, B'101' AS b
-- columns: a: str (BIT)
-- columns: b: str (BIT)
```

## Checks

```python
import decimal
import typing
from sqlty import SQL

q = SQL[tuple[decimal.Decimal]]("--sql\nSELECT total FROM orders")
meta = SQL[tuple[list[str]]]("--sql\nSELECT '[]'::jsonb")
many = SQL[tuple[list[str], str]]("--sql\nSELECT ARRAY[name]::text[], '{}'::jsonb FROM users")
# error: [stale-type] "SQL[tuple[float]] should be SQL[tuple[decimal.Decimal]]"
wrong = SQL[tuple[float]]("--sql\nSELECT total FROM orders")
```

## Rewrite adds the imports

```python
q = "--sql\nSELECT total, '[]'::jsonb, '1.2.3.4'::inet FROM orders"
# rewritten-file: import decimal
# rewritten-file: import ipaddress
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[decimal.Decimal, typing.Any, ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface]]("--sql\nSELECT total, '[]'::jsonb, '1.2.3.4'::inet FROM orders")
```

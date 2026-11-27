# Text arguments PostgreSQL rejects

psycopg sends a `str` as text of unknown type, and PostgreSQL reads it with the input
function of the parameter's type. A `str` literal, or a name bound once to one, is checked
as the server reads it, so text it rejects is reported before the query runs.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TYPE status AS ENUM ('active', 'pending');
CREATE DOMAIN positive AS integer CHECK (VALUE > 0);
CREATE TABLE users (
    id integer PRIMARY KEY,
    small smallint,
    big bigint,
    price numeric(10, 2),
    score real,
    ratio double precision,
    admin boolean,
    ref uuid,
    born date,
    alarm time,
    seen timestamp,
    created_at timestamptz,
    timeout interval,
    status status,
    tier text CHECK (tier IN ('free', 'pro')),
    rank positive
);
```

## Each type

```python
import psycopg

conn = psycopg.connect("")
# error: [param-type-mismatch] "parameter 1 is 'abc', which PostgreSQL rejects as integer"
conn.execute("SELECT id FROM users WHERE id = %s", ("abc",))
# error: [param-type-mismatch] "parameter 1 is '70000', which PostgreSQL rejects as smallint"
conn.execute("SELECT id FROM users WHERE small = %s", ("70000",))
# error: [param-type-mismatch] "parameter 1 is '1.5', which PostgreSQL rejects as bigint"
conn.execute("SELECT id FROM users WHERE big = %s", ("1.5",))
# error: [param-type-mismatch] "parameter 1 is '1,50', which PostgreSQL rejects as numeric"
conn.execute("SELECT id FROM users WHERE price = %s", ("1,50",))
# error: [param-type-mismatch] "parameter 1 is 'high', which PostgreSQL rejects as real"
conn.execute("SELECT id FROM users WHERE score = %s", ("high",))
# error: [param-type-mismatch] "parameter 1 is '1_0.5', which PostgreSQL rejects as double precision"
conn.execute("SELECT id FROM users WHERE ratio = %s", ("1_0.5",))
# error: [param-type-mismatch] "parameter 1 is 'o', which PostgreSQL rejects as boolean"
conn.execute("SELECT id FROM users WHERE admin = %s", ("o",))
# error: [param-type-mismatch] "parameter 1 is 'a0eebc99', which PostgreSQL rejects as uuid"
conn.execute("SELECT id FROM users WHERE ref = %s", ("a0eebc99",))
# error: [param-type-mismatch] "parameter 1 is '2026-02-30', which PostgreSQL rejects as date"
conn.execute("SELECT id FROM users WHERE born = %s", ("2026-02-30",))
# error: [param-type-mismatch] "parameter 1 is '25:00', which PostgreSQL rejects as time"
conn.execute("SELECT id FROM users WHERE alarm = %s", ("25:00",))
# error: [param-type-mismatch] "parameter 1 is '2026-11-26 25:00', which PostgreSQL rejects as timestamp"
conn.execute("SELECT id FROM users WHERE seen = %s", ("2026-11-26 25:00",))
# error: [param-type-mismatch] "parameter 1 is 'not a date', which PostgreSQL rejects as timestamp with time zone"
conn.execute("SELECT id FROM users WHERE created_at > %s", ("not a date",))
# error: [param-type-mismatch] "parameter 1 is 'forever', which PostgreSQL rejects as interval"
conn.execute("SELECT id FROM users WHERE timeout = %s", ("forever",))
```

## Text the server accepts

```python
import psycopg

conn = psycopg.connect("")
conn.execute("SELECT id FROM users WHERE id = %s AND small = %s AND big = %s", ("1", " 42 ", "0x1F"))
conn.execute("SELECT id FROM users WHERE price = %s AND score = %s AND ratio = %s", ("1_000.50", "NaN", "-Infinity"))
conn.execute("SELECT id FROM users WHERE admin = %s AND ref = %s", ("yes", "{A0EEBC99-9C0B4EF8-BB6D6BB9-BD380A11}"))
conn.execute("SELECT id FROM users WHERE born = %s AND alarm = %s AND seen = %s", ("2026-11-26", "allballs", "yesterday"))
conn.execute("SELECT id FROM users WHERE created_at > %s AND born < %s", ("infinity", "January 8, 99 BC"))
conn.execute("SELECT id FROM users WHERE timeout = %s AND seen = %s", ("1 day 2 hours ago", "2026-11-26T07:00:00.5"))
# error:
```

## A name bound once to text

```python
import psycopg

ID = "abc"
OTHER = "1"
OTHER = "x"

def find(conn: psycopg.Connection, given: str = "abc") -> None:
    local = "2026-13-01"
    # error: [param-type-mismatch] "parameter 1 is 'abc', which PostgreSQL rejects as integer"
    conn.execute("SELECT id FROM users WHERE id = %s", (ID,))
    # error: [param-type-mismatch] "parameter 1 is '2026-13-01', which PostgreSQL rejects as date"
    conn.execute("SELECT id FROM users WHERE born = %s", (local,))
    conn.execute("SELECT id FROM users WHERE id = %s", (OTHER,))
    conn.execute("SELECT id FROM users WHERE id = %s", (given,))

def reassigned(conn: psycopg.Connection, value: str) -> None:
    value = "abc"
    conn.execute("SELECT id FROM users WHERE id = %s", (value,))

def twice(conn: psycopg.Connection, flag: bool) -> None:
    value = "abc"
    if flag:
        value += "1"
    conn.execute("SELECT id FROM users WHERE id = %s", (value,))
```

## Enum and `CHECK` values

```python
import psycopg

conn = psycopg.connect("")
# error: [param-type-mismatch] "parameter 1 is 'deleted', not one of 'active', 'pending'"
conn.execute("SELECT id FROM users WHERE status = %s", ("deleted",))
# error: [param-type-mismatch] "parameter 'tier' is 'Pro', not one of 'free', 'pro'"
conn.execute("SELECT id FROM users WHERE tier = %(tier)s", {"tier": "Pro"})
conn.execute("SELECT id FROM users WHERE status = %s AND tier = %s", ("active", "pro"))
```

## Casts and domains

An explicit cast converts the text in SQL, and a domain reads it as its base type: its
`CHECK` applies to stored values only.

```python
import psycopg

conn = psycopg.connect("")
conn.execute("SELECT id FROM users WHERE id::text = %s::text", ("abc",))
conn.execute("SELECT id FROM users WHERE rank = %s", ("-1",))
# error: [param-type-mismatch] "parameter 1 is 'one', which PostgreSQL rejects as integer"
conn.execute("SELECT id FROM users WHERE rank = %s", ("one",))
```

## MySQL and SQLite are not judged

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id integer PRIMARY KEY, born date);
```

```python
import psycopg

conn = psycopg.connect("")
conn.execute("SELECT id FROM users WHERE id = %s AND born = %s", ("abc", "not a date"))
# error:
```

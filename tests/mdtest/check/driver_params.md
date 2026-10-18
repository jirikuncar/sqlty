# Check arguments of driver execute calls

`execute`, `executemany` and the asyncpg fetch methods are checked when the file
shows what the receiver is: a variable bound to `psycopg.connect(...)`,
`sqlite3.connect(...)` or `asyncpg.connect(...)`, to `<connection>.cursor()`,
or annotated with a driver class. The query is a literal, `SQL[T]("...")` of
one, or a name assigned once to one.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## psycopg cursor and connection

```python
import psycopg

with psycopg.connect("dbname=x") as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM users WHERE id = %s", (1,))
        # error: [param-type-mismatch] "parameter 1 expects int, got bytes"
        cur.execute("SELECT id FROM users WHERE id = %s", (b"1",))
        # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
        cur.execute("SELECT id FROM users WHERE id = %s AND age = %s", (1,))
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        cur.execute("SELECT id FROM users WHERE id = %s")
        cur.execute("SELECT id FROM users")
    # error: [param-type-mismatch] "parameter 2 is not nullable, got None"
    conn.execute("INSERT INTO users (id, name) VALUES (%s, %s)", (1, None))
    # error: [param-type-mismatch] "parameter 'id' expects int, got bytes"
    conn.execute("DELETE FROM users WHERE id = %(id)s", {"id": b"1"})
    # error: [param-type-mismatch] "parameter 1 expects int, got bytes"
    conn.execute("DELETE FROM users WHERE id = %s", params=(b"1",))
```

## executemany checks each row

```python
import psycopg

conn = psycopg.connect("dbname=x")
cur = conn.cursor()
cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2, "b")])
# error:
cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2.5, "b")])
```

## A query bound to a name or wrapped in SQL

```python
import psycopg
from sqlty import SQL

conn = psycopg.connect("dbname=x")
QUERY = "DELETE FROM users WHERE id = %s"
WRAPPED = SQL[tuple[()]]("--sql\nDELETE FROM users WHERE id = %s")
# error: [param-type-mismatch] "parameter 1 expects int, got bytes"
conn.execute(QUERY, (b"1",))
# error: [param-type-mismatch] "parameter 1 expects int, got bytes"
conn.execute(WRAPPED, (b"1",))
```

## Annotated receivers

```python
import psycopg

def remove(cur: psycopg.Cursor, age: bytes) -> None:
    # error: [param-type-mismatch] "parameter 1 expects int, got bytes"
    cur.execute("DELETE FROM users WHERE age = %s", (age,))
```

## Unrecognised receivers and unknown arguments are skipped

```python
import psycopg

def remove(db, params, other, *rest) -> None:
    db.execute("DELETE FROM users WHERE id = %s")
    db.execute("DELETE FROM users WHERE id = %s", (b"1",))

conn = psycopg.connect("dbname=x")
conn.execute("DELETE FROM users WHERE id = %s", params_variable)
conn.execute("DELETE FROM users WHERE id = %s", (*rest_args,))
conn.execute(query_variable, (b"1",))
# error:
```

## asyncpg

```python
import asyncpg

async def main() -> None:
    conn = await asyncpg.connect("postgres://x")
    await conn.fetch("SELECT id FROM users WHERE id = $1", 1)
    # error: [param-type-mismatch] "parameter 1 expects int, got bytes"
    await conn.fetch("SELECT id FROM users WHERE id = $1", b"1")
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await conn.execute("DELETE FROM users WHERE id = $1 AND age = $2", 1)
```

### A parameter shifted by an interval

PostgreSQL resolves `$1 - INTERVAL '3 months'` before the comparison around it: an
untyped `$1` becomes an `interval`, and comparing that with a `timestamptz` fails.
psycopg sends the type of a `datetime`, so the query works there, but asyncpg sends
parameters untyped. A cast fixes it.

`schema.sql`:

```sql
CREATE TABLE keys (id INTEGER PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL);
```

```python
import datetime

import asyncpg
import psycopg

async def main(now: datetime.datetime) -> None:
    conn = await asyncpg.connect("postgres://x")
    # error: [param-type-mismatch] "parameter 1 is an `interval` to PostgreSQL, as asyncpg sends it untyped, so it cannot be compared with a `timestamptz`: cast it, `$1::timestamptz`"
    await conn.execute("DELETE FROM keys WHERE created_at < $1 - INTERVAL '3 months'", now)
    await conn.execute("DELETE FROM keys WHERE created_at < $1::timestamptz - INTERVAL '3 months'", now)
    with psycopg.connect("dbname=x") as sync:
        sync.execute("DELETE FROM keys WHERE created_at < %s - INTERVAL '3 months'", (now,))
```

## Names are resolved per scope

A module-level connection does not make a loop variable, a parameter or a comprehension variable
one, and a rebinding in one function leaves the others alone.

```python
import psycopg

conn = psycopg.connect("dbname=x")

def other(conns, conn2, conn3: "Fake"):
    for conn in conns:
        conn.execute("DELETE FROM users WHERE id = %s")
    conn2.execute("DELETE FROM users WHERE id = %s")
    conn3.execute("DELETE FROM users WHERE id = %s")

def shadowed(conn):
    conn.execute("DELETE FROM users WHERE id = %s")

def fake():
    conn = Fake()
    conn.execute("DELETE FROM users WHERE id = %s")

def comprehension(conns):
    return [c.execute("DELETE FROM users WHERE id = %s") for c in conns]

def with_other():
    with open_something() as conn:
        conn.execute("DELETE FROM users WHERE id = %s")

def inner():
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    conn.execute("DELETE FROM users WHERE id = %s")
```

### One scope does not leak into another

```python
import psycopg

def first():
    conn = psycopg.connect("dbname=x")
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    conn.execute("DELETE FROM users WHERE id = %s")

def second():
    conn = Fake()
    conn.execute("DELETE FROM users WHERE id = %s")
```

### Rebinding makes a name unknown

```python
import psycopg

conn = psycopg.connect("dbname=x")
conn = Fake()
conn.execute("DELETE FROM users WHERE id = %s")
# error:
```

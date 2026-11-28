# Check arguments of rustmy execute calls

[rustmy](https://github.com/M4RC0Sx/rustmy) takes `%s` and `%(name)s` placeholders
in `execute` and `executemany`, on its blocking and its `rustmy.aio` cursors. Unlike
psycopg, it binds a single value to a query's one placeholder, a `str` included.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## A cursor of a connection

```python
import rustmy

with rustmy.connect(host="localhost", database="app") as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM users WHERE id = %s", (1,))
        # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
        cur.execute("SELECT id FROM users WHERE id = %s AND age = %s", (1,))
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        cur.execute("SELECT id FROM users WHERE id = %s")
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        cur.execute("SELECT id FROM users WHERE id = %s", None)
        # error: [param-type-mismatch] "parameter 2 is not nullable, got None"
        cur.execute("INSERT INTO users (id, name) VALUES (%s, %s)", (1, None))
        # error: [extra-param] "call passes extra parameter 'ident'"
        # error: [param-count-mismatch] "call is missing parameter 'id'"
        cur.execute("DELETE FROM users WHERE id = %(id)s", {"ident": 1})
        # error: [param-type-mismatch] "parameter 1 expects int, got bytes"
        cur.execute("DELETE FROM users WHERE id = %s", args=(b"1",))
```

## A single value

```python
import rustmy

def find(cur: rustmy.Cursor, user_id: int, name: str) -> None:
    cur.execute("SELECT id FROM users WHERE id = %s", user_id)
    cur.execute("SELECT id FROM users WHERE name = %s", name)
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    cur.execute("SELECT id FROM users WHERE id = %s AND name = %s", user_id)
    # error: [param-type-mismatch] "parameter 1 expects str, got int"
    cur.execute("SELECT id FROM users WHERE name = %s", user_id)
    # error: [param-type-mismatch] "parameters are a single int, which rustmy binds only to `%s`: pass a mapping"
    cur.execute("SELECT id FROM users WHERE id = %(id)s", user_id)
    # error: [param-type-mismatch] "parameters are a single int, which rustmy binds only to `%s`: pass a mapping"
    cur.executemany("DELETE FROM users WHERE id = %(id)s", [1])
```

## executemany checks each row

A row may be a single value too.

```python
import rustmy

conn = rustmy.connect(host="localhost", database="app")
cur = conn.cursor()
cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2, "b")])
cur.executemany("DELETE FROM users WHERE id = %s", [1, 2])
# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2,)])
# error: [param-count-mismatch] "query has 1 parameter, the call passes 2"
cur.executemany("DELETE FROM users WHERE id = %s", seq_of_args=[(1, 2)])
```

## Cursor classes and prepared cursors

```python
import rustmy
from rustmy.cursors import DictCursor, PreparedCursor

def run(conn: rustmy.Connection, prepared: PreparedCursor) -> None:
    rows = conn.cursor(DictCursor)
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    rows.execute("SELECT id FROM users WHERE id = %s")
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 2"
    prepared.execute("SELECT id FROM users WHERE id = %s", (1, 2))
```

## asyncio and pools

`rustmy.aio` connections, cursors and pools, and the connections a pool lends.

```python
from rustmy import aio

async def main() -> None:
    conn = await aio.connect(host="localhost", database="app")
    async with conn.cursor() as cur:
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        await cur.execute("SELECT id FROM users WHERE id = %s")
    async with aio.create_pool(host="localhost", database="app") as pool:
        async with pool.acquire() as leased, leased.cursor() as cursor:
            # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
            await cursor.execute("SELECT id FROM users WHERE id = %s AND age = %s", (1,))

async def find(cur: aio.DictCursor) -> None:
    # error: [param-type-mismatch] "parameter 1 expects int, got bytes"
    await cur.execute("SELECT id FROM users WHERE id = %s", b"1")
```

## A blocking pool

```python
import rustmy

pool = rustmy.Pool(host="localhost", database="app")
with pool.acquire() as conn, conn.cursor() as cur:
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    cur.execute("SELECT id FROM users WHERE id = %s")
```

## Imported from the defining modules

```python
from rustmy.aio.connections import connect
from rustmy.aio.pool import create_pool

async def main() -> None:
    conn = await connect(host="localhost", database="app")
    pool = await create_pool(host="localhost", database="app")
    async with conn.cursor() as cur, pool.acquire() as leased:
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        await cur.execute("SELECT id FROM users WHERE id = %s")
        async with leased.cursor() as other:
            # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
            await other.execute("SELECT id FROM users WHERE id = %s")
```

## The `sqlty.drivers.rustmy` helpers

`execute`, `scalar` and their `_async` versions are checked like the cursor methods. A
single value is the parameters of `execute`; the one argument after the query of
`scalar` is its parameters only when it is a literal of them, as it may be the row type.

```python
import rustmy
from rustmy import aio
from sqlty import SQL
from sqlty.drivers import rustmy as typed
from sqlty.drivers.rustmy import execute, execute_async, scalar, scalar_async

BY_ID = SQL[tuple[str]]("-- sql\nSELECT name FROM users WHERE id = %s AND age = %s")

def run(conn: rustmy.Connection, ids: tuple[int, int]) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    execute(conn, BY_ID, (1,))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    execute(conn, BY_ID, params=(1,))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 0"
    typed.execute(conn, BY_ID)
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    execute(conn, BY_ID, 1)
    execute(conn, "SELECT name FROM users WHERE id = %s", 1)
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    scalar(conn, BY_ID, (1,))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    scalar(conn, "SELECT name FROM users WHERE id = %s AND age = %s", str, (1,))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    scalar(conn, "SELECT name FROM users WHERE id = %s AND age = %s", str, 1)
    execute(conn, BY_ID, ids)
    scalar(conn, BY_ID, ids)
    scalar(conn, BY_ID.bind((1, 2)))

async def run_async(conn: aio.Connection) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await execute_async(conn, BY_ID, (1,))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 0"
    await scalar_async(conn, BY_ID)
```

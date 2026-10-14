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
        # error: [param-type-mismatch] "parameter 1 expects int, got float"
        cur.execute("SELECT id FROM users WHERE id = %s", (1.5,))
        # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
        cur.execute("SELECT id FROM users WHERE id = %s AND age = %s", (1,))
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        cur.execute("SELECT id FROM users WHERE id = %s")
        cur.execute("SELECT id FROM users")
    # error: [param-type-mismatch] "parameter 2 is not nullable, got None"
    conn.execute("INSERT INTO users (id, name) VALUES (%s, %s)", (1, None))
    # error: [param-type-mismatch] "parameter 'id' expects int, got float"
    conn.execute("DELETE FROM users WHERE id = %(id)s", {"id": 1.5})
    # error: [param-type-mismatch] "parameter 1 expects int, got float"
    conn.execute("DELETE FROM users WHERE id = %s", params=(1.5,))
```

## executemany checks each row

```python
import psycopg

conn = psycopg.connect("dbname=x")
cur = conn.cursor()
cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2, "b")])
# error: [param-type-mismatch] "parameter 1 expects int, got float"
cur.executemany("INSERT INTO users (id, name) VALUES (%s, %s)", [(1, "a"), (2.5, "b")])
```

## A query bound to a name or wrapped in SQL

```python
import psycopg
from sqlty import SQL

conn = psycopg.connect("dbname=x")
QUERY = "DELETE FROM users WHERE id = %s"
WRAPPED = SQL[tuple[()]]("--sql\nDELETE FROM users WHERE id = %s")
# error: [param-type-mismatch] "parameter 1 expects int, got float"
conn.execute(QUERY, (1.5,))
# error: [param-type-mismatch] "parameter 1 expects int, got float"
conn.execute(WRAPPED, (1.5,))
```

## Annotated receivers

```python
import psycopg

def remove(cur: psycopg.Cursor, age: float) -> None:
    # error: [param-type-mismatch] "parameter 1 expects int, got float"
    cur.execute("DELETE FROM users WHERE age = %s", (age,))
```

## Unrecognised receivers and unknown arguments are skipped

```python
import psycopg

def remove(db, params, other, *rest) -> None:
    db.execute("DELETE FROM users WHERE id = %s")
    db.execute("DELETE FROM users WHERE id = %s", (1.5,))

conn = psycopg.connect("dbname=x")
conn.execute("DELETE FROM users WHERE id = %s", params_variable)
conn.execute("DELETE FROM users WHERE id = %s", (*rest_args,))
conn.execute(query_variable, (1.5,))
# error:
```

## asyncpg

```python
import asyncpg

async def main() -> None:
    conn = await asyncpg.connect("postgres://x")
    await conn.fetch("SELECT id FROM users WHERE id = $1", 1)
    # error: [param-type-mismatch] "parameter 1 expects int, got float"
    await conn.fetch("SELECT id FROM users WHERE id = $1", 1.5)
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await conn.execute("DELETE FROM users WHERE id = $1 AND age = $2", 1)
```

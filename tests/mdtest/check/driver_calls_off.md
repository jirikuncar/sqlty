# Driver calls turned off

`driver-calls = false` leaves connection and cursor calls alone. The
`sqlty.drivers.psycopg` helpers and `SQL[T]` queries are still checked; a
`SQL[T]` passed to a connection method has its parameters counted only through the
helpers.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
driver-calls = false
```

## Connection methods are skipped

```python
import psycopg

with psycopg.connect("dbname=x") as conn:
    conn.execute("SELECT id FROM users WHERE id = %s", (b"1",))
    conn.cursor().execute("SELECT id FROM users WHERE id = %s AND age = %s", (1,))
```

## Helpers and marked queries are checked

```python
import psycopg
from sqlty import SQL
from sqlty.drivers.psycopg import execute

BY_ID = SQL[tuple[str]]("--sql\nSELECT name FROM users WHERE id = %s AND age = %s")

def run(conn: psycopg.Connection) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    execute(conn, BY_ID, (1,))
    conn.execute(BY_ID, (1,))
```

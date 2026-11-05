# Check arguments of aiosqlite execute calls

aiosqlite hands the parameters to sqlite3 unchanged, so sqlite3's rules apply.

`schema.sql`:

```sql
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## Annotated connection

```python
import aiosqlite

async def save(db: aiosqlite.Connection) -> None:
    await db.execute("INSERT INTO meta (key, value) VALUES (?, ?)", ("version", "1"))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute("INSERT INTO meta (key, value) VALUES (?, ?)", ("version",))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute_insert("INSERT INTO meta (key, value) VALUES (?, ?)", ("version",))
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    await db.execute_fetchall("SELECT value FROM meta WHERE key = ?")
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.executemany("INSERT INTO meta (key, value) VALUES (?, ?)", [("a", "1"), ("b",)])
    # error: [param-type-mismatch] "parameter 2 is not nullable, got None"
    await db.execute("INSERT INTO meta (key, value) VALUES (?, ?)", ("version", None))
    # error: [param-count-mismatch] "call is missing parameter 'key'"
    await db.execute("SELECT value FROM meta WHERE key = :key", {"name": "a"})
```

## Imported class names

```python
from aiosqlite import Connection, Cursor

async def save(db: Connection, cur: Cursor) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute("INSERT INTO meta (key, value) VALUES (?, ?)", ("version",))
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    await cur.execute("SELECT value FROM meta WHERE key = ?")
```

## Awaited and async with receivers

```python
import aiosqlite

async def main() -> None:
    db = await aiosqlite.connect("app.db")
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute("INSERT INTO meta (key, value) VALUES (?, ?)", ("version",))
    cur = await db.cursor()
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    await cur.execute("SELECT value FROM meta WHERE key = ?")

async def other() -> None:
    async with aiosqlite.connect("app.db") as db:
        async with db.cursor() as cur:
            # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
            await cur.executemany("INSERT INTO meta (key, value) VALUES (?, ?)", [("a",)])
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        async with db.execute("SELECT value FROM meta WHERE key = ?") as rows:
            pass
```

## A single value is not a sequence of parameters

```python
import aiosqlite

async def find(db: aiosqlite.Connection, key: str, n: int) -> None:
    await db.execute("SELECT value FROM meta WHERE key = ?", key)
    # error: [param-type-mismatch] "parameters are a single int, which aiosqlite rejects"
    await db.execute("SELECT value FROM meta WHERE key = ?", n)
```

## Keyword parameters

```python
import aiosqlite

async def save(db: aiosqlite.Connection) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute("INSERT INTO meta (key, value) VALUES (?, ?)", parameters=("version",))
```

## Other methods and receivers are skipped

```python
import aiosqlite

async def run(db: aiosqlite.Connection, other) -> None:
    await db.executescript("INSERT INTO meta (key, value) VALUES (?, ?)")
    await other.execute("INSERT INTO meta (key, value) VALUES (?, ?)", ("version",))
# error:
```

## Also configured as a helper

A driver method registered in `[[tool.sqlty.functions]]` has its parameters
reported once.

```toml
[[tool.sqlty.functions]]
import-path = "aiosqlite.core.Connection.execute"
query-param = "sql"
```

`aiosqlite/core.py`:

```python
from typing import Any

class Connection:
    async def execute(self, sql: str, parameters: Any = None) -> Any: ...
```

```python
from aiosqlite.core import Connection

async def save(db: Connection) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute("--sql\nINSERT INTO meta (key, value) VALUES (?, ?)", ("version",))
```

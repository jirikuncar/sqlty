# Check arguments of aiosqlite execute calls

aiosqlite hands the parameters of `execute`, `executemany`, `execute_fetchall`
and `execute_insert` to sqlite3 as they are, so its calls are checked as
sqlite3's are.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## An awaited connection

```python
import aiosqlite

async def main() -> None:
    db = await aiosqlite.connect("app.db")
    await db.execute("SELECT id FROM users WHERE id = ?", (1,))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute("SELECT id FROM users WHERE id = ? AND age = ?", (1,))
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    await db.execute_fetchall("SELECT id FROM users WHERE id = ?")
    # error: [param-type-mismatch] "parameter 2 is not nullable, got None"
    await db.execute_insert("INSERT INTO users (id, name) VALUES (?, ?)", (1, None))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.execute_insert("INSERT INTO users (id, name) VALUES (?, ?)", parameters=(1,))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await db.executemany("INSERT INTO users (id, name) VALUES (?, ?)", [(1, "a"), (2,)])
```

## A connection and a cursor in `async with`

```python
import aiosqlite

async def main() -> None:
    async with aiosqlite.connect("app.db") as db:
        # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
        async with db.execute("SELECT id FROM users WHERE id = ?") as cursor:
            await cursor.fetchall()
        async with db.cursor() as cur:
            # error: [param-count-mismatch] "query has 1 parameter, the call passes 2"
            await cur.execute("SELECT id FROM users WHERE id = ?", (1, 2))
```

## An awaited cursor

```python
import aiosqlite

async def main() -> None:
    db = await aiosqlite.connect("app.db")
    cursor = await db.cursor()
    await cursor.execute("SELECT id FROM users WHERE id = ?", (1,))
    # error: [extra-param] "call passes extra parameter 'ident'"
    # error: [param-count-mismatch] "call is missing parameter 'id'"
    await cursor.execute("SELECT id FROM users WHERE id = :id", {"ident": 1})
```

## Annotated receivers

A connection passed to a function is recognised by its annotation.

```python
import aiosqlite
from aiosqlite import Connection

async def find(db: aiosqlite.Connection, user_id: int, name: str) -> None:
    await db.execute_fetchall("SELECT id FROM users WHERE id = ?", (user_id,))
    await db.execute_fetchall("SELECT id FROM users WHERE name = ?", name)
    # error: [param-type-mismatch] "parameters are a single int, which aiosqlite rejects"
    await db.execute_fetchall("SELECT id FROM users WHERE id = ?", user_id)

async def remove(db: Connection) -> None:
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    await db.execute("DELETE FROM users WHERE id = ?")

async def update(cur: aiosqlite.Cursor) -> None:
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await cur.execute("UPDATE users SET age = ? WHERE id = ?", (1,))
```

## Unrecognised receivers are skipped

```python
import aiosqlite

async def remove(db, other: "aiosqlite.Connection") -> None:
    await db.execute("DELETE FROM users WHERE id = ?")
    await other.execute("DELETE FROM users WHERE id = ?")

class Repository:
    def __init__(self, db: aiosqlite.Connection) -> None:
        self.db = db

    async def remove(self) -> None:
        await self.db.execute("DELETE FROM users WHERE id = ?")
# error:
```

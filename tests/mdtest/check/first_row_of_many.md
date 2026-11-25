# first-row-of-many

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL UNIQUE, team_id BIGINT, name TEXT NOT NULL);
CREATE TABLE memberships (user_id BIGINT NOT NULL, team_id BIGINT NOT NULL, PRIMARY KEY (user_id, team_id));
```

## psycopg cursor

```python
import psycopg

conn = psycopg.connect("")
cur = conn.cursor()
# error: [first-row-of-many] "fetchone() reads the first row of a query that can return many"
cur.execute("SELECT name FROM users WHERE team_id = %s", (1,)).fetchone()
cur.execute("SELECT name FROM users WHERE id = %s", (1,)).fetchone()
cur.execute("SELECT name FROM users ORDER BY id LIMIT 1").fetchone()
cur.execute("SELECT 1 FROM memberships WHERE user_id = %s AND team_id = %s", (1, 2)).fetchone()
# error: [first-row-of-many] "fetchone() reads the first row of a query that can return many"
cur.execute("SELECT 1 FROM memberships WHERE user_id = %s", (1,)).fetchone()
cur.execute("SELECT name FROM users WHERE team_id = %s", (1,)).fetchall()
```

## Bundled scalar

```python
from typing import Any

from sqlty import SQL
from sqlty.drivers.psycopg import scalar


def first(conn: Any) -> str:
    # error: [first-row-of-many] "scalar() reads the first row of a query that can return many"
    return scalar(conn, SQL[tuple[str]]("--sql\nSELECT name FROM users"))


def count(conn: Any) -> int:
    return scalar(conn, SQL[tuple[int]]("--sql\nSELECT count(*)::INT FROM users"))
```

## asyncpg

```python
import asyncpg


async def run() -> None:
    conn = await asyncpg.connect("")
    # error: [first-row-of-many] "fetchrow() reads the first row of a query that can return many"
    await conn.fetchrow("SELECT name FROM users WHERE name = $1", "a")
    # error: [first-row-of-many] "fetchval() reads the first row of a query that can return many"
    await conn.fetchval("SELECT id FROM users")
    await conn.fetchrow("SELECT name FROM users WHERE email = $1", "a")
    await conn.fetch("SELECT name FROM users")
```

## aiosqlite awaited cursor

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
import aiosqlite


async def run() -> None:
    db = await aiosqlite.connect("")
    # error: [first-row-of-many] "fetchone() reads the first row of a query that can return many"
    await (await db.execute("SELECT name FROM users", ())).fetchone()
    await (await db.execute("SELECT name FROM users WHERE id = ?", (1,))).fetchone()
```

## Unknown keys are not reported

```python
import psycopg

conn = psycopg.connect("")
conn.execute("SELECT x FROM missing WHERE y = %s", (1,)).fetchone()
conn.execute("WITH u AS (SELECT name FROM users) SELECT name FROM u").fetchone()
conn.execute("SELECT name FROM users UNION SELECT email FROM users").fetchone()
```

## Ignored

```toml
[tool.sqlty.rules]
first-row-of-many = "ignore"
```

```python
import psycopg

conn = psycopg.connect("")
# error:
conn.execute("SELECT name FROM users").fetchone()
```

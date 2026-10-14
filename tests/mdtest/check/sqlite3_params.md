# Check arguments of sqlite3 execute calls

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## Connection and cursor

```python
import sqlite3

conn = sqlite3.connect("app.db")
cur = conn.cursor()
cur.execute("SELECT id FROM users WHERE id = ?", (1,))
cur.execute("SELECT id FROM users WHERE id = ?", (b"1",))
# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
conn.execute("SELECT id FROM users WHERE id = ? AND age = ?", (1,))
# error: [param-type-mismatch] "parameter 2 is not nullable, got None"
conn.execute("INSERT INTO users (id, name) VALUES (?, ?)", (1, None))
```

## Named placeholders

```python
import sqlite3

with sqlite3.connect("app.db") as conn:
    conn.execute("SELECT id FROM users WHERE id = :id", {"id": 1})
    # error: [param-count-mismatch] "call is missing parameter 'id'"
    conn.execute("SELECT id FROM users WHERE id = :id", {"ident": 1})
    # SQLite is dynamically typed: no type is wrong, but NULL is for NOT NULL.
    conn.execute("SELECT id FROM users WHERE id = :id", {"id": b"1", "extra": 2})
```

## executemany

```python
import sqlite3

conn = sqlite3.connect("app.db")
# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
conn.executemany("INSERT INTO users (id, name) VALUES (?, ?)", [(1, "a"), (2,)])
```

## Types are not checked, NULL and counts are

SQLite columns hold any type, so a `bytes` or `date` for an `INTEGER` column is not reported.

```python
import sqlite3
from datetime import date

conn = sqlite3.connect("app.db")
conn.execute("SELECT id FROM users WHERE id = ?", (date(2020, 1, 1),))
conn.execute("SELECT id FROM users WHERE id = :id", {"id": b"1"})
# error:
```

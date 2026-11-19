# Receivers

A registered method is found by the receiver's name only when sqlty cannot type
the receiver, or types it as `Any` or `object`.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Untyped receivers

```toml
[[tool.sqlty.functions]]
import-path = "postgres.Postgres.one"
query-param = "sql"
receivers = ["db", "cursor"]
```

```python
from typing import Any

class Participant:
    db: Any

    def name(self) -> str:
        # error: [unknown-column]
        return self.db.one("SELECT nme FROM users")

def first(cursor: Any, other: Any) -> None:
    # error: [unknown-column]
    cursor.one("SELECT nme FROM users")
    other.one("SELECT nme FROM users")
```

## Typed receivers

A receiver of a known class calls that class's method, not the registered one.

```toml
[[tool.sqlty.functions]]
import-path = "postgres.Postgres.one"
query-param = "sql"
receivers = ["db"]
```

```python
class Cache:
    def one(self, key: str) -> str:
        return key

class Service:
    def __init__(self, db: Cache) -> None:
        self.db = db

    def get(self) -> str:
        return self.db.one("user:1")

def get(db: Cache) -> str:
    return db.one("user:1")
```

## Query not first

The definition gives the query's position.

```toml
[[tool.sqlty.functions]]
import-path = "postgres.Postgres.one"
query-param = "sql"
receivers = ["db"]
```

`postgres.py`:

```python
from typing import Any

class Postgres:
    def one(self, params: Any, sql: str) -> Any: ...
```

```python
from typing import Any

def name(db: Any) -> None:
    # error: [unknown-column]
    db.one((1,), "SELECT nme FROM users WHERE id = %s")
```

## First entry wins

```toml
[[tool.sqlty.functions]]
import-path = "postgres.Postgres.one"
query-param = "sql"
receivers = ["db"]

[[tool.sqlty.functions]]
import-path = "cache.Cache.one"
query-param = "key"
receivers = ["db"]
```

```python
from typing import Any

def name(db: Any) -> None:
    # error: [unknown-column]
    db.one("SELECT nme FROM users")
```

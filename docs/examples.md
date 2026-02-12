# Examples

Working examples, from simplest to most advanced.

---

## 1. Simple Tuple Types (Rewrite/Strip)

The simplest approach — write `--sql` queries, run `sqlty rewrite .` to add types.

```python
# queries.py
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"
```

```bash
sqlty rewrite queries.py
```

```python
# queries.py — after rewrite
from sqlty import SQL

user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
count_query = SQL[tuple[int]]("--sql\nSELECT COUNT(*)::INTEGER FROM users")
```

```bash
sqlty check queries.py    # verify annotations
sqlty strip queries.py    # revert to plain strings
```

---

## 2. User-Defined Types (TypedDict, dataclass, etc.)

Use your own types instead of `tuple[...]` for readable, named fields:

```python
# queries.py
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")
```

`sqlty check` validates that `User` fields match the query columns — count, types, and names.

### All supported forms

```python
from typing import TypedDict, NamedTuple
from dataclasses import dataclass
from sqlty import SQL

# TypedDict
class UserTD(TypedDict):
    id: int
    name: str

# NamedTuple
class UserNT(NamedTuple):
    id: int
    name: str

# dataclass
@dataclass
class UserDC:
    id: int
    name: str

# Pydantic BaseModel (optional dependency)
from pydantic import BaseModel
class UserBM(BaseModel):
    id: int
    name: str
```

All four forms work the same way with `SQL[T]` and `sqlty check`.

---

## 3. Schema-Based Inference

Use a database schema for type inference — no explicit `::TYPE` casts needed.

```toml
# pyproject.toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);
```

```python
# queries.py — no casts needed
user_query = "--sql\nSELECT id, name, email FROM users"
```

```bash
sqlty rewrite --schema schema.sql queries.py
```

```python
# queries.py — types inferred from schema
from sqlty import SQL

user_query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

---

## 4. Magic Codec

Import-time transformation — `--sql` strings are automatically rewritten when imported.

**Location:** [`examples/magic_codec/`](https://github.com/jirikuncar/sqlty/tree/main/examples/magic_codec)

```python
# coding: sqlty

# --sql strings transformed at import time (no sqlty rewrite needed)
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# At runtime: SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The codec is registered automatically when sqlty is installed. See [magic codec docs](magic-codec.md).

---

## 5. Datetime and UUID Handling

Types like `datetime` and `uuid` are resolved at runtime, so import style doesn't matter:

```python
from datetime import datetime, date
from uuid import UUID
from typing import TypedDict
from sqlty import SQL

class Event(TypedDict):
    id: int
    event_date: date       # matches ::DATE
    created_at: datetime   # matches ::TIMESTAMP
    token: UUID            # matches ::UUID

query = SQL[Event](
    "--sql\nSELECT id::INTEGER, event_date::DATE, created_at::TIMESTAMP, token::UUID FROM events"
)
# sqlty check: all types match correctly
```

---

## Running Examples

```bash
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty && uv sync

# Rewrite, check, strip
sqlty rewrite examples/magic_codec/
sqlty check examples/magic_codec/
sqlty strip examples/magic_codec/
```

## Pre-commit Integration

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-check
        name: sqlty check
        entry: uv run sqlty check .
        language: system
        pass_filenames: false
        types: [python]
      - id: sqlty-rewrite
        name: sqlty rewrite
        entry: uv run sqlty rewrite .
        language: system
        pass_filenames: false
        types: [python]
```

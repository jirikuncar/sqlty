# Examples

Working examples, from simplest to most advanced. Each example is a self-contained directory in [`examples/`](https://github.com/jirikuncar/sqlty/tree/main/examples).

---

## 1. Quick Start — Rewrite/Strip/Check

**Directory:** [`examples/01_quick_start/`](https://github.com/jirikuncar/sqlty/tree/main/examples/01_quick_start)

Write `--sql` queries with explicit type casts, run `sqlty rewrite` to add annotations.

```python
# queries.py
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"
```

```bash
sqlty rewrite examples/01_quick_start/
```

`queries.py` after the rewrite (each `# rewritten-file:` line is one line of the output):

```python
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten-file: count_query = SQL[tuple[int]]("--sql\nSELECT COUNT(*)::INTEGER FROM users")
```

```bash
sqlty check examples/01_quick_start/    # verify annotations
sqlty strip examples/01_quick_start/    # revert to plain strings
```

---

## 2. Schema-Based Inference

**Directory:** [`examples/02_schema_inference/`](https://github.com/jirikuncar/sqlty/tree/main/examples/02_schema_inference)

Use a `CREATE TABLE` schema file so SQLTy infers types without explicit `::TYPE` casts.

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
sqlty rewrite --schema schema.sql examples/02_schema_inference/
```

```python
# queries.py — types inferred from schema
from sqlty import SQL

user_query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

---

## 3. pyproject.toml Configuration

**Directory:** [`examples/03_pyproject_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/03_pyproject_config)

Configure dialect, schema, and lint rules in `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
column-count-mismatch = "error"
column-type-mismatch = "error"
column-name-mismatch = "warn"
select-star = "warn"
stale-rewrite = "warn"
```

CLI flags override config values:

```bash
sqlty check .                          # uses pyproject.toml config
sqlty check --dialect sqlite .         # overrides dialect
sqlty rewrite --schema other.sql .     # overrides schema
```

---

## 4. Check and User-Defined Types

**Directory:** [`examples/04_check_and_types/`](https://github.com/jirikuncar/sqlty/tree/main/examples/04_check_and_types)

Use `TypedDict`, `NamedTuple`, `dataclass`, or Pydantic `BaseModel` instead of `tuple[...]`:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")
```

`sqlty check` validates field count, types, and names against the query.

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

## 5. Magic Codec

**Directory:** [`examples/magic_codec/`](https://github.com/jirikuncar/sqlty/tree/main/examples/magic_codec)

Import-time transformation — `--sql` strings are automatically rewritten when imported.

```python
# coding: sqlty

user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

No `sqlty rewrite` step needed. Best for development and prototyping.

---

## 6. Datetime and UUID Handling

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

## 7. psycopg3 Row Factory Integration

Use `SQL[T]` with psycopg3's row factories for end-to-end type safety — sqlty validates at dev time, psycopg3 maps rows at runtime.

### Before: psycopg3 only (runtime errors on mismatch)

```python
from psycopg.rows import class_row
from dataclasses import dataclass

@dataclass
class User:
    id: int
    name: str
    email: str

# If SQL drifts from User (column added, type changed), you find out at runtime
with psycopg.connect("dbname=mydb") as conn:
    cur = conn.cursor(row_factory=class_row(User))
    cur.execute("SELECT id, name, email FROM users")
    users = cur.fetchall()  # Runtime error if columns don't match User
```

### After: sqlty + psycopg3 (dev-time validation)

```python
import psycopg
from dataclasses import dataclass
from sqlty import SQL
from sqlty.drivers.psycopg import execute

@dataclass
class User:
    id: int
    name: str
    email: str

# sqlty check: validates SQL return type matches User at dev time
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")

with psycopg.connect("dbname=mydb") as conn:
    users = execute(conn, query).fetchall()  # list[User], via class_row(User)
```

### With dict_row for ad-hoc queries

```python
from psycopg.rows import dict_row
from sqlty import SQL

query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")

with psycopg.connect("dbname=mydb") as conn:
    cur = conn.cursor(row_factory=dict_row)
    cur.execute(query)
    rows = cur.fetchall()  # list[dict[str, Any]] at runtime
    # sqlty still validates the SQL types match tuple[int, str]
```

---

## Running Examples

```bash
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty && uv sync

# Quick start
sqlty rewrite examples/01_quick_start/
sqlty check examples/01_quick_start/
sqlty strip examples/01_quick_start/

# Schema inference
sqlty rewrite --schema examples/02_schema_inference/schema.sql examples/02_schema_inference/

# Magic codec
python examples/magic_codec/demo.py
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

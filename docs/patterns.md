# Usage Patterns

Three ways to use SQLTy, from simplest to most automated.

## Pattern 1: Rewrite/Strip (Recommended)

Write `--sql\n` strings, use `sqlty rewrite` to add type annotations, `sqlty strip` to remove them.

```python
# 1. Write queries with --sql prefix and type casts
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
```

```bash
# 2. Rewrite to add types
sqlty rewrite .
```

```python
# 3. Result
from sqlty import SQL
query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

```bash
# 4. Check and strip
sqlty check .     # lint for issues
sqlty strip .     # revert to plain strings
```

**Best for:** Production codebases, CI/CD, team workflows.

### Pre-commit integration

```yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-check
        name: sqlty check
        entry: uv run sqlty check .
        language: system
        pass_filenames: false
        types: [python]
```

## Pattern 2: User-Defined Types

Use `TypedDict`, `NamedTuple`, `dataclass`, or Pydantic `BaseModel` instead of `tuple[...]`:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

`sqlty check` validates field count, types, and names against the query.

**Best for:** Readable code, named fields, complex queries with many columns.

## Pattern 3: Schema-Based Inference

Use a `CREATE TABLE` schema file to infer types without explicit casts:

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
# No ::TYPE casts needed
query = "--sql\nSELECT id, name, email FROM users"

# After rewrite:
query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

**Best for:** Projects with existing schema files.

## Pattern 4: Magic Codec

Import-time transformation — no `sqlty rewrite` step needed:

```python
# coding: sqlty

query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# At runtime: SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

**Best for:** Development, prototyping, rapid iteration.

See [magic codec documentation](magic-codec.md).

## Pattern 5: psycopg3 Row Factories

`SQL[T]` is a `str` subclass, so it passes directly to psycopg3's `execute()`. Combine with `class_row` for end-to-end type safety:

```python
import psycopg
from psycopg.rows import class_row
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str
    email: str

# sqlty validates the SQL ↔ Python type mapping at dev time
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")

with psycopg.connect("dbname=mydb") as conn:
    cur = conn.cursor(row_factory=class_row(User))
    cur.execute(query)
    users: list[User] = cur.fetchall()
```

**How the pieces fit together:**

| Layer | Responsibility | When |
|-------|---------------|------|
| `sqlty check` | SQL ↔ Python type match | dev time |
| `SQL[T]` | Carries type parameter | type checking |
| `class_row(T)` | Maps DB rows → Python objects | runtime |

Without sqlty, a mismatch between SQL columns and `User` fields surfaces only at runtime. With sqlty, `sqlty check` catches it before the code runs.

### Multiple queries with schema inference

With a schema file, you skip explicit `::TYPE` casts:

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
import psycopg
from psycopg.rows import class_row, dict_row
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str
    email: str

# Types inferred from schema — no casts needed
get_users = SQL[User]("--sql\nSELECT id, name, email FROM users")
get_stats = SQL[tuple[int]]("--sql\nSELECT COUNT(*) FROM users")

with psycopg.connect("dbname=mydb") as conn:
    # Typed rows via class_row
    users = conn.cursor(row_factory=class_row(User)).execute(get_users).fetchall()

    # Tuple rows for simple aggregates
    (count,) = conn.execute(get_stats).fetchone()
```

**Best for:** Production codebases using psycopg3 that want compile-time SQL validation.

## Which Pattern?

| Pattern | Best for |
|---------|----------|
| **Rewrite/Strip** | Production, CI/CD, teams |
| **User-Defined Types** | Readable code, named fields |
| **Schema-Based** | Projects with CREATE TABLE schemas |
| **Magic Codec** | Development, prototyping |
| **psycopg3 Row Factories** | psycopg3 codebases, end-to-end type safety |

All patterns combine freely. Use magic codec during development, rewrite before committing, user-defined types for important domain objects.

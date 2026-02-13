# Advanced Usage

## SQL Dialects

SQLTy supports multiple SQL dialects via sqlglot:

```bash
sqlty rewrite . --dialect postgres   # default
sqlty rewrite . --dialect mysql
sqlty rewrite . --dialect sqlite
```

Or configure in `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "sqlite"
```

## Multiple Schemas

Provide a single schema via CLI:

```bash
sqlty rewrite --schema schema.sql .
sqlty check --schema schema.sql .
```

Or configure named schemas:

```toml
[tool.sqlty.schemas]
users = "db/users.sql"
orders = "db/orders.sql"
```

### Schema Precedence

1. `--schema <PATH>` CLI flag (highest priority)
2. `[tool.sqlty] schema = "..."` in `pyproject.toml`
3. `[tool.sqlty.schemas]` named registries

## Formatting Rewritten Files

Pipe rewritten output through an external formatter:

```toml
[tool.sqlty]
format-command = "ruff format --stdin-filename {filename} -"
```

## Lint Rule Configuration

```toml
[tool.sqlty.rules]
select-star = "ignore"           # suppress SELECT * warnings
column-name-mismatch = "error"   # treat name mismatches as errors
stale-rewrite = "warn"           # warn on unwrapped --sql strings
```

Valid severities: `"error"`, `"warn"`, `"ignore"`.

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

## User-Defined Type Validation

`sqlty check` validates `SQL[UserType]` annotations structurally:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Checks field count, types, and names. Uses Python's runtime typing machinery for correct resolution of `from datetime import datetime` and similar import patterns.

Supported: `TypedDict`, `NamedTuple`, `@dataclass`, Pydantic `BaseModel`.

**Limitation:** type must be defined in the same file as the `SQL[T](...)` usage.

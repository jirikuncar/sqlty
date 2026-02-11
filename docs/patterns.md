# Usage Patterns

This guide shows the main patterns for using SQLTy in your projects.

## Pattern 1: Rewrite/Strip (Recommended)

The primary pattern — write `--sql` prefixed strings, use `sqlty rewrite` to add type annotations.

### Workflow

```python
# 1. Write queries with --sql prefix and type casts
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
```

```bash
# 2. Rewrite to add types
sqlty rewrite .
```

```python
# 3. Result: type-safe SQL strings
from sqlty import SQL
query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

```bash
# 4. Check for issues
sqlty check .

# 5. Strip when you need to edit SQL
sqlty strip .
```

### Benefits

- No decorators or wrappers needed in your code
- Bidirectional: `rewrite` adds types, `strip` removes them
- Works with any codebase — just add `--sql` prefix to your SQL strings
- `check` validates annotations stay in sync

### Configuration

```toml
# pyproject.toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
select-star = "warn"
```

### Pre-commit Integration

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-check
        name: sqlty check
        entry: uv run sqlty check
        language: system
        pass_filenames: false
        types: [python]
      - id: sqlty-rewrite
        name: sqlty rewrite
        entry: uv run sqlty rewrite
        language: system
        pass_filenames: false
        types: [python]
```

## Pattern 2: Magic Codec

Import-time transformation — no explicit `sqlty rewrite` step needed.

```python
# coding: sqlty

# --sql strings are automatically transformed at import time
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# At runtime: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Benefits

- Zero manual steps — transformation happens at import
- Great for development and prototyping
- Types are available at runtime without pre-processing

### Trade-offs

- Import-time overhead (parsing and type inference)
- Types are not visible in source code (only at runtime)
- Requires codec registration via `sqlty_magic.pth`

See [magic codec documentation](magic-codec.md) for setup.

## Pattern 3: Schema-Based Inference

Use a database schema file to infer types without explicit casts:

```toml
# pyproject.toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```python
# No explicit casts needed — types inferred from schema
query = "--sql\nSELECT id, name, email FROM users"

# After rewrite:
query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

### Multiple Schemas

```toml
[tool.sqlty.schemas]
users = "db/users.sql"
orders = "db/orders.sql"
```

## Which Pattern Should I Use?

| Pattern | Best for |
|---------|----------|
| **Rewrite/Strip** | Production codebases, CI/CD, team workflows |
| **Magic Codec** | Development, prototyping, quick iteration |
| **Schema-Based** | Projects with existing CREATE TABLE schemas |

All patterns can be combined. For example, use the magic codec during development and `sqlty rewrite` before committing.
